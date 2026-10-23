//! Board support for the M5Stack Cardputer ADV on esp-hal.
//!
//! - [`display`]: the 1.14" ST7789V2 LCD as a 240×135 RGB565
//!   embedded-graphics `DrawTarget` (landscape, keyboard below), backlight on.
//! - [`keyboard`]: the 56-key keyboard (TCA8418 on I2C), with Shift/Fn layers
//!   from [`cardputer_adv_keyboard`].
//!
//! Pins (from the ADV schematic and working firmware):
//!
//! | Function | GPIO |
//! |---|---|
//! | LCD SCK / MOSI / CS / DC / RST | 36 / 35 / 37 / 34 / 33 |
//! | LCD backlight (also powers the RGB LED) | 38 |
//! | I2C SDA / SCL (keyboard, ES8311, BMI270) | 8 / 9 |
//! | Keyboard interrupt | 11 |

#![no_std]

pub use cardputer_adv_keyboard as keys;
use embedded_hal_bus::spi::{ExclusiveDevice, NoDelay};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c},
    peripherals::{GPIO8, GPIO9, GPIO33, GPIO34, GPIO35, GPIO36, GPIO37, GPIO38, I2C0, SPI2},
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

/// The keyboard on the board's I2C bus.
pub type Keyboard = keys::Keyboard<I2c<'static, Blocking>>;

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
}

/// Initialises the keyboard. It takes the I2C bus for itself (the codec
/// and IMU on the same bus are unused here).
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
    keys::Keyboard::new(i2c).map_err(|_| Error::Keyboard)
}
