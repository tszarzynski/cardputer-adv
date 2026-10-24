//! Board support for the M5Stack Cardputer ADV on esp-hal.
//!
//! - [`display`]: the 1.14" ST7789V2 LCD as a 240×135 RGB565
//!   embedded-graphics `DrawTarget` (landscape, keyboard below), backlight on.
//! - [`keyboard`]: the 56-key keyboard (TCA8418 on I2C), with Shift/Fn layers
//!   from [`cardputer_adv_keyboard`]. [`Keyboard::wait_inputs`] sleeps until
//!   the TCA8418 signals a key event on its interrupt line.
//! - [`battery`]: the battery voltage. The board can't tell whether it's
//!   charging.
//!
//! Pins (from the ADV schematic and working firmware):
//!
//! | Function | GPIO |
//! |---|---|
//! | LCD SCK / MOSI / CS / DC / RST | 36 / 35 / 37 / 34 / 33 |
//! | LCD backlight (also powers the RGB LED) | 38 |
//! | I2C SDA / SCL (keyboard, ES8311, BMI270) | 8 / 9 |
//! | Keyboard interrupt | 11 |
//! | Battery voltage (ADC1, through a 2:1 divider) | 10 |

#![no_std]

pub use cardputer_adv_keyboard as keys;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use embedded_hal_bus::spi::{ExclusiveDevice, NoDelay};
use esp_hal::{
    Blocking,
    delay::Delay,
    analog::adc::{Adc, AdcCalCurve, AdcConfig, AdcPin, Attenuation},
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    i2c::master::{Config as I2cConfig, I2c},
    peripherals::{
        ADC1, GPIO8, GPIO9, GPIO10, GPIO11, GPIO33, GPIO34, GPIO35, GPIO36, GPIO37, GPIO38,
        I2C0, SPI2,
    },
    spi::{
        Mode,
        master::{Config as SpiConfig, Spi},
    },
    time::Rate,
};
use mipidsi::{
    Builder,
    interface::SpiInterface,
    models::ST7789,
    options::{ColorInversion, Orientation, Rotation},
};
use static_cell::StaticCell;

/// Display width and height in landscape.
pub const WIDTH: u16 = 240;
pub const HEIGHT: u16 = 135;

type DisplaySpi = ExclusiveDevice<Spi<'static, Blocking>, Output<'static>, NoDelay>;

/// The LCD: an embedded-graphics `DrawTarget<Color = Rgb565>`.
pub type Display =
    mipidsi::Display<SpiInterface<'static, DisplaySpi, Output<'static>>, ST7789, Output<'static>>;

/// The peripherals the LCD uses.
pub struct DisplayPins {
    pub spi: SPI2<'static>,
    pub sck: GPIO36<'static>,
    pub mosi: GPIO35<'static>,
    pub cs: GPIO37<'static>,
    pub dc: GPIO34<'static>,
    pub reset: GPIO33<'static>,
    pub backlight: GPIO38<'static>,
}

#[derive(Debug)]
pub enum Error {
    Spi,
    I2c,
    DisplayInit,
    Keyboard,
}

/// Initialises the LCD (landscape, cleared to black by the caller) and turns
/// the backlight on. Keep the returned backlight pin alive; dropping it turns
/// the backlight off.
///
/// # Errors
///
/// If the SPI bus or the panel can't be set up.
///
/// # Panics
///
/// If called twice (the SPI transfer buffer is a `static`).
pub fn display(pins: DisplayPins) -> Result<(Display, Output<'static>), Error> {
    static BUFFER: StaticCell<[u8; 512]> = StaticCell::new();

    let backlight = Output::new(pins.backlight, Level::High, OutputConfig::default());
    let dc = Output::new(pins.dc, Level::Low, OutputConfig::default());
    let reset = Output::new(pins.reset, Level::High, OutputConfig::default());
    let cs = Output::new(pins.cs, Level::High, OutputConfig::default());
    let spi = Spi::new(
        pins.spi,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(40))
            .with_mode(Mode::_0),
    )
    .map_err(|_| Error::Spi)?
    .with_sck(pins.sck)
    .with_mosi(pins.mosi);
    let device = ExclusiveDevice::new_no_delay(spi, cs).map_err(|_| Error::Spi)?;
    let interface = SpiInterface::new(device, dc, BUFFER.init([0; 512]));
    let display = Builder::new(ST7789, interface)
        .reset_pin(reset)
        .display_size(HEIGHT, WIDTH)
        .display_offset(52, 40)
        .orientation(Orientation::new().rotate(Rotation::Deg90))
        .invert_colors(ColorInversion::Inverted)
        .init(&mut Delay::new())
        .map_err(|_| Error::DisplayInit)?;
    Ok((display, backlight))
}

/// The peripherals the keyboard uses.
pub struct KeyboardPins {
    pub i2c: I2C0<'static>,
    pub sda: GPIO8<'static>,
    pub scl: GPIO9<'static>,
    /// The TCA8418's interrupt output (active low).
    pub interrupt: GPIO11<'static>,
}

/// How often to look for key events anyway, in case an interrupt is missed
/// (or the line doesn't work): key presses are then late, not lost.
const FALLBACK_POLL: Duration = Duration::from_secs(1);

/// The pause after a wake-up with nothing to read. If the line is stuck low
/// this turns a busy loop into slow polling, so other tasks keep running.
const SPURIOUS_BACKOFF: Duration = Duration::from_millis(10);

/// The most key inputs one read returns (the TCA8418's FIFO holds 10
/// events).
const MAX_INPUTS: usize = 10;

/// The keyboard: the TCA8418 driver and its interrupt line.
pub struct Keyboard {
    keys: keys::Keyboard<I2c<'static, Blocking>>,
    interrupt: Input<'static>,
    /// Events arrived while the last batch was being read: read again
    /// without waiting.
    pending: bool,
    missed: u32,
}

/// Why [`Keyboard::wait_inputs`] went to read the keyboard.
enum Wake {
    Interrupt,
    Fallback,
    Pending,
}

impl Keyboard {
    /// Waits for key presses and returns them (with Shift/Fn applied).
    ///
    /// Sleeps until the TCA8418 pulls its interrupt line low, so an idle
    /// keyboard costs nothing. It waits for the low *level*, not an edge:
    /// events that arrive while the caller is busy keep the line low.
    ///
    /// # Errors
    ///
    /// If the TCA8418 can't be read over I2C.
    pub async fn wait_inputs(&mut self) -> Result<impl Iterator<Item = keys::KeyInput>, Error> {
        loop {
            let wake = if core::mem::take(&mut self.pending) {
                Wake::Pending
            } else {
                match select(self.interrupt.wait_for_low(), Timer::after(FALLBACK_POLL)).await {
                    Either::First(()) => Wake::Interrupt,
                    Either::Second(()) => Wake::Fallback,
                }
            };
            let events = self.keys.events().map_err(|_| Error::Keyboard)?;
            // Clear every flag (key events, overflow, …) so no source keeps
            // the line low, then catch an event that landed in between. The
            // keys are already read, so a failure here mustn't lose them:
            // read again on the next call instead.
            let cleared = self.keys.clear_interrupts().is_ok();
            self.pending = !cleared || self.keys.events_available().unwrap_or(true);

            let mut any_event = false;
            let mut inputs = [None; MAX_INPUTS];
            for (slot, input) in inputs.iter_mut().zip(events.filter_map(|event| {
                any_event = true;
                event.input
            })) {
                *slot = Some(input);
            }
            if !any_event {
                // Woken with nothing to read: back off, so a line stuck low
                // can't keep this task from yielding.
                if matches!(wake, Wake::Interrupt) {
                    Timer::after(SPURIOUS_BACKOFF).await;
                }
                continue;
            }
            if matches!(wake, Wake::Fallback) {
                // Events without an interrupt: the line isn't working.
                self.missed = self.missed.saturating_add(1);
            }
            // Releases (and lone modifiers) are events without an input.
            if inputs[0].is_none() {
                continue;
            }
            return Ok(inputs.into_iter().flatten());
        }
    }

    /// How many times key events were found only by the fallback poll, which
    /// points at a broken interrupt line.
    #[must_use]
    pub const fn missed_interrupts(&self) -> u32 {
        self.missed
    }

    /// The underlying driver, for anything [`wait_inputs`](Self::wait_inputs)
    /// doesn't cover.
    pub const fn driver(&mut self) -> &mut keys::Keyboard<I2c<'static, Blocking>> {
        &mut self.keys
    }
}

/// Initialises the keyboard and its interrupt. It takes the I2C bus for
/// itself (the codec and IMU on the same bus are unused here).
///
/// # Errors
///
/// If the bus or the TCA8418 can't be set up.
pub fn keyboard(pins: KeyboardPins) -> Result<Keyboard, Error> {
    let i2c = I2c::new(
        pins.i2c,
        I2cConfig::default().with_frequency(Rate::from_khz(400)),
    )
    .map_err(|_| Error::I2c)?
    .with_sda(pins.sda)
    .with_scl(pins.scl);
    let mut keys = keys::Keyboard::new(i2c).map_err(|_| Error::Keyboard)?;
    keys.enable_keyboard_interrupt()
        .map_err(|_| Error::Keyboard)?;
    keys.clear_interrupts().map_err(|_| Error::Keyboard)?;
    // The TCA8418's INT is open-drain.
    let interrupt = Input::new(pins.interrupt, InputConfig::default().with_pull(Pull::Up));
    Ok(Keyboard {
        keys,
        interrupt,
        pending: false,
        missed: 0,
    })
}

/// The peripherals the battery measurement uses.
pub struct BatteryPins {
    pub adc: ADC1<'static>,
    /// The battery, through the board's 2:1 divider.
    pub pin: GPIO10<'static>,
}

/// How many readings [`Battery::millivolts`] averages.
const BATTERY_SAMPLES: u32 = 16;

/// The battery voltage, measured as M5Stack's M5Unified does for the
/// Cardputer and Cardputer ADV: GPIO10 on ADC1, through a 2:1 divider.
pub struct Battery {
    adc: Adc<'static, ADC1<'static>, Blocking>,
    pin: AdcPin<GPIO10<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
}

impl Battery {
    /// The battery voltage in millivolts, averaged over a few readings.
    ///
    /// The board can't tell whether it's charging: on USB with the power
    /// switch on, this is the charging voltage, close to full.
    pub fn millivolts(&mut self) -> u16 {
        let sum: u32 = (0..BATTERY_SAMPLES)
            .map(|_| u32::from(self.adc.read_blocking(&mut self.pin)))
            .sum();
        u16::try_from(sum / BATTERY_SAMPLES * 2).unwrap_or(u16::MAX)
    }
}

/// Sets up the battery measurement: ADC1 with the chip's calibration curve,
/// at 11 dB attenuation (up to about 3.1 V at the pin, 6.2 V at the battery).
#[must_use]
pub fn battery(pins: BatteryPins) -> Battery {
    let mut config = AdcConfig::new();
    let pin = config.enable_pin_with_cal::<_, AdcCalCurve<ADC1<'static>>>(
        pins.pin,
        Attenuation::_11dB,
    );
    Battery {
        adc: Adc::new(pins.adc, config),
        pin,
    }
}
