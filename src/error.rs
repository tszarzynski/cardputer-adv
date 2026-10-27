//! What can fail.

use core::fmt;

/// What failed. The underlying driver errors aren't kept: on this board,
/// each part has one way to fail that matters (it didn't answer, or the
/// peripheral couldn't be configured).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The LCD's SPI bus couldn't be configured.
    Spi,
    /// The I2C bus couldn't be configured.
    I2c,
    /// The LCD didn't take its initialisation sequence.
    DisplayInit,
    /// The TCA8418 keyboard controller didn't answer.
    Keyboard,
    /// The backlight's PWM couldn't be configured.
    Backlight,
    /// The motion sensor wasn't found, or didn't start.
    Imu,
    /// The I2S output couldn't be started, or restarted after running dry.
    Speaker,
    /// The ES8311 codec didn't answer.
    Codec,
    /// A frame couldn't be sent to the LCD.
    DisplayWrite,
    /// The I2S input from the microphone couldn't be started, or restarted
    /// after overflowing.
    Microphone,
    /// The RMT (for the RGB LED and the IR transmitter) couldn't be set up.
    Rmt,
    /// A colour couldn't be sent to the RGB LED.
    Led,
    /// An IR code couldn't be sent.
    Ir,
    /// The microSD card's SPI bus couldn't be set up.
    SdCard,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Spi => "LCD SPI bus setup failed",
            Self::I2c => "I2C bus setup failed",
            Self::DisplayInit => "LCD initialisation failed",
            Self::Keyboard => "keyboard controller (TCA8418) not answering",
            Self::Backlight => "backlight PWM setup failed",
            Self::Imu => "motion sensor (BMI270) not found or not starting",
            Self::Speaker => "speaker I2S stream failed",
            Self::Codec => "audio codec (ES8311) not answering",
            Self::DisplayWrite => "LCD frame write failed",
            Self::Microphone => "microphone I2S stream failed",
            Self::Rmt => "RMT setup failed",
            Self::Led => "RGB LED (WS2812) write failed",
            Self::Ir => "IR transmission failed",
            Self::SdCard => "microSD SPI bus setup failed",
        })
    }
}

impl core::error::Error for Error {}
