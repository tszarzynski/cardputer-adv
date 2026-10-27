//! Macros that take each part's pins out of esp-hal's `Peripherals`, as
//! the board wires them. A macro, not a function, so that only those
//! fields move and the rest of `Peripherals` stays usable.

/// The LCD's peripherals ([`DisplayPins`]) from esp-hal's `Peripherals`,
/// for [`display`] or [`frame_display`].
///
/// ```ignore
/// let (display, backlight) = cardputer_adv::display(cardputer_adv::display_pins!(p))?;
/// ```
///
/// [`DisplayPins`]: crate::DisplayPins
/// [`display`]: crate::display
/// [`frame_display`]: crate::frame_display
#[macro_export]
macro_rules! display_pins {
    ($peripherals:expr) => {
        $crate::DisplayPins {
            spi: $peripherals.SPI2,
            sck: $peripherals.GPIO36,
            mosi: $peripherals.GPIO35,
            cs: $peripherals.GPIO37,
            dc: $peripherals.GPIO34,
            reset: $peripherals.GPIO33,
            backlight: $peripherals.GPIO38,
            ledc: $peripherals.LEDC,
        }
    };
}

/// The shared I2C bus's peripherals ([`I2cPins`]) from esp-hal's
/// `Peripherals`, for [`i2c`].
///
/// [`I2cPins`]: crate::I2cPins
/// [`i2c`]: crate::i2c
#[macro_export]
macro_rules! i2c_pins {
    ($peripherals:expr) => {
        $crate::I2cPins {
            i2c: $peripherals.I2C0,
            sda: $peripherals.GPIO8,
            scl: $peripherals.GPIO9,
        }
    };
}

/// The keyboard's peripherals ([`KeyboardPins`]) from esp-hal's
/// `Peripherals`, for [`keyboard`].
///
/// [`KeyboardPins`]: crate::KeyboardPins
/// [`keyboard`]: crate::keyboard
#[macro_export]
macro_rules! keyboard_pins {
    ($peripherals:expr) => {
        $crate::KeyboardPins {
            i2c: $crate::i2c_pins!($peripherals),
            interrupt: $peripherals.GPIO11,
        }
    };
}

/// The speaker's peripherals ([`SpeakerPins`]) from esp-hal's
/// `Peripherals`, for [`speaker`]. Its stream takes DMA channel 0.
///
/// [`SpeakerPins`]: crate::SpeakerPins
/// [`speaker`]: crate::speaker
#[macro_export]
macro_rules! speaker_pins {
    ($peripherals:expr) => {
        $crate::SpeakerPins {
            i2s: $peripherals.I2S0,
            dma: $peripherals.DMA_CH0,
            bclk: $peripherals.GPIO41,
            ws: $peripherals.GPIO43,
            dout: $peripherals.GPIO42,
        }
    };
}

/// The speaker's and the microphone's peripherals ([`AudioPins`]) from
/// esp-hal's `Peripherals`, for [`audio`]. Their streams take DMA
/// channel 0.
///
/// [`AudioPins`]: crate::AudioPins
/// [`audio`]: crate::audio
#[macro_export]
macro_rules! audio_pins {
    ($peripherals:expr) => {
        $crate::AudioPins {
            speaker: $crate::speaker_pins!($peripherals),
            din: $peripherals.GPIO46,
        }
    };
}

/// The battery measurement's peripherals ([`BatteryPins`]) from esp-hal's
/// `Peripherals`, for [`battery`].
///
/// [`BatteryPins`]: crate::BatteryPins
/// [`battery`]: crate::battery
#[macro_export]
macro_rules! battery_pins {
    ($peripherals:expr) => {
        $crate::BatteryPins {
            adc: $peripherals.ADC1,
            pin: $peripherals.GPIO10,
        }
    };
}

/// The microSD card's peripherals ([`SdCardPins`]) from esp-hal's
/// `Peripherals`, for [`sd_card`].
///
/// [`SdCardPins`]: crate::SdCardPins
/// [`sd_card`]: crate::sd_card
#[cfg(feature = "sd-card")]
#[macro_export]
macro_rules! sd_card_pins {
    ($peripherals:expr) => {
        $crate::SdCardPins {
            spi: $peripherals.SPI3,
            sck: $peripherals.GPIO40,
            mosi: $peripherals.GPIO14,
            miso: $peripherals.GPIO39,
            cs: $peripherals.GPIO12,
        }
    };
}
