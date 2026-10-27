//! The LCD drawn on directly, and its backlight.

use core::fmt;

use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_hal::digital::OutputPin;
use embedded_hal_bus::spi::{ExclusiveDevice, NoDelay};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{DriveMode, Level, Output, OutputConfig},
    ledc::{
        LSGlobalClkSource, Ledc, LowSpeed,
        channel::{self, ChannelIFace},
        timer::{self, TimerIFace},
    },
    peripherals::{GPIO33, GPIO34, GPIO35, GPIO36, GPIO37, GPIO38, LEDC, SPI2},
    spi::{
        Mode,
        master::{Config as SpiConfig, Spi},
    },
    time::Rate,
};
use mipidsi::{
    Builder,
    interface::{Interface, InterfacePixelFormat, SpiInterface},
    models::ST7789,
    options::{ColorInversion, Orientation, Rotation},
};
use static_cell::StaticCell;

use crate::Error;

/// Display width in landscape.
pub const WIDTH: u16 = 240;
/// Display height in landscape.
pub const HEIGHT: u16 = 135;

type DisplaySpi = ExclusiveDevice<Spi<'static, Blocking>, Output<'static>, NoDelay>;

/// The LCD: an embedded-graphics `DrawTarget<Color = Rgb565>`.
pub type Display =
    mipidsi::Display<SpiInterface<'static, DisplaySpi, Output<'static>>, ST7789, Output<'static>>;

/// The peripherals the LCD uses.
#[expect(missing_docs, reason = "the fields are the pins they're named after")]
#[derive(Debug)]
pub struct DisplayPins {
    pub spi: SPI2<'static>,
    pub sck: GPIO36<'static>,
    pub mosi: GPIO35<'static>,
    pub cs: GPIO37<'static>,
    pub dc: GPIO34<'static>,
    pub reset: GPIO33<'static>,
    pub backlight: GPIO38<'static>,
    /// Drives the backlight with PWM, for dimming.
    pub ledc: LEDC<'static>,
}

/// The LCD backlight, dimmable by PWM.
///
/// It's 256 Hz from LEDC low-speed channel 0, as M5Stack's M5GFX drives it
/// on the Cardputer. Its LED driver can't
/// follow fast PWM: at 24 kHz, 15% duty looked off. GPIO38 also powers the
/// RGB LED, which dims with it.
pub struct Backlight {
    channel: channel::Channel<'static, LowSpeed>,
}

impl Backlight {
    /// Sets the brightness, 0 (off) to 100 (full).
    ///
    /// # Errors
    ///
    /// If LEDC rejects the duty (it can't, for 0–100).
    pub fn set_percent(&mut self, percent: u8) -> Result<(), Error> {
        self.channel
            .set_duty(percent.min(100))
            .map_err(|_| Error::Backlight)
    }
}

/// Starts the backlight at full brightness.
///
/// # Panics
///
/// If called twice (the PWM timer is a `static`).
pub fn backlight(ledc: LEDC<'static>, pin: GPIO38<'static>) -> Result<Backlight, Error> {
    static TIMER: StaticCell<timer::Timer<'static, LowSpeed>> = StaticCell::new();

    let mut ledc = Ledc::new(ledc);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let timer = TIMER.init(ledc.timer::<LowSpeed>(timer::Number::Timer0));
    timer
        .configure(timer::config::Config {
            // 80 MHz / (256 Hz × 1024 steps): a divider LEDC can do.
            duty: timer::config::Duty::Duty10Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_hz(256),
        })
        .map_err(|_| Error::Backlight)?;
    let mut channel = ledc.channel(channel::Number::Channel0, pin);
    channel
        .configure(channel::config::Config {
            timer,
            duty_pct: 100,
            drive_mode: DriveMode::PushPull,
        })
        .map_err(|_| Error::Backlight)?;
    Ok(Backlight { channel })
}

/// Initialises the LCD (landscape, cleared by the caller) and turns the
/// backlight on at full brightness.
///
/// # Errors
///
/// If the SPI bus or the panel can't be set up.
///
/// # Panics
///
/// If called twice (the SPI transfer buffer and the PWM timer are `static`).
pub fn display(pins: DisplayPins) -> Result<(Display, Backlight), Error> {
    static BUFFER: StaticCell<[u8; 512]> = StaticCell::new();

    let backlight = backlight(pins.ledc, pins.backlight)?;
    let dc = Output::new(pins.dc, Level::Low, OutputConfig::default());
    let reset = Output::new(pins.reset, Level::High, OutputConfig::default());
    let cs = Output::new(pins.cs, Level::High, OutputConfig::default());
    let spi = Spi::new(pins.spi, spi_config())
        .map_err(|_| Error::Spi)?
        .with_sck(pins.sck)
        .with_mosi(pins.mosi);
    let device = ExclusiveDevice::new_no_delay(spi, cs).map_err(|_| Error::Spi)?;
    let interface = SpiInterface::new(device, dc, BUFFER.init([0; 512]));
    let display = init_panel(interface, reset)?;
    Ok((display, backlight))
}

/// The LCD's SPI bus: 40 MHz, as M5GFX runs it.
pub fn spi_config() -> SpiConfig {
    SpiConfig::default()
        .with_frequency(Rate::from_mhz(40))
        .with_mode(Mode::_0)
}

/// Where the panel's 135×240 pixels sit in the ST7789's 240×320 memory.
pub const PANEL_OFFSET: (u16, u16) = (52, 40);

/// Initialises the panel in landscape (keyboard below), colours inverted.
pub fn init_panel<DI, RST>(
    interface: DI,
    reset: RST,
) -> Result<mipidsi::Display<DI, ST7789, RST>, Error>
where
    DI: Interface,
    Rgb565: InterfacePixelFormat<DI::Word>,
    RST: OutputPin,
{
    Builder::new(ST7789, interface)
        .reset_pin(reset)
        .display_size(HEIGHT, WIDTH)
        .display_offset(PANEL_OFFSET.0, PANEL_OFFSET.1)
        .orientation(Orientation::new().rotate(Rotation::Deg90))
        .invert_colors(ColorInversion::Inverted)
        .init(&mut Delay::new())
        .map_err(|_| Error::DisplayInit)
}

impl fmt::Debug for Backlight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Backlight").finish_non_exhaustive()
    }
}
