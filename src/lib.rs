//! Board support for the M5Stack Cardputer ADV on esp-hal.
//!
//! - [`display`]: the 1.14" ST7789V2 LCD as a 240×135 RGB565
//!   embedded-graphics `DrawTarget` (landscape, keyboard below), and its
//!   [`Backlight`], dimmable by PWM. [`frame_display`] feeds it from a
//!   [`Frame`] by DMA instead.
//! - [`keyboard`]: the 56-key keyboard (TCA8418 on I2C). [`Keyboard::wait_for_inputs`]
//!   sleeps until the TCA8418 signals a key event on its interrupt line,
//!   and returns what the keys type, with Shift and Fn applied ([`keys`]).
//! - [`imu`]: the BMI270 motion sensor: its accelerometer, in milli-g, and
//!   its gyroscope, in milli-degrees a second.
//! - [`speaker`]: the speaker, fed with samples over I2S (the ES8311 codec,
//!   then an NS4150B amplifier). [`audio`] starts it with the microphone,
//!   as an [`Audio`]. [`codec`] turns the codec on and returns the
//!   [`Codec`], which sets its level, and turns its output and the
//!   microphone off and on.
//! - [`i2c`]: the I2C bus the keyboard, the codec and the motion sensor
//!   share. Only needed to use more than the keyboard; [`keyboard`] alone
//!   sets it up itself.
//! - [`battery`]: the battery voltage, and the charge left as a percentage.
//!   The board can't tell whether it's charging.
//! - [`button`]: the G0 button.
//! - [`rmt`]: the RMT peripheral, for the [`rgb_led`] (a WS2812) and the
//!   [`ir_transmitter`] (NEC codes).
//! - `sd_card` (with the `sd-card` feature): the microSD card, through
//!   [embedded-sdmmc](https://crates.io/crates/embedded-sdmmc).
//!
//! Each part's pins come out of esp-hal's `Peripherals` with a macro named
//! after it, [`display_pins!`], [`keyboard_pins!`] and so on; only those
//! fields move, so the rest stays usable.
//!
//! The async parts need an embassy-time driver, such as esp-rtos's.
//!
//! # Quick start
//!
//! ```ignore
//! use cardputer_adv::{display_pins, i2c_pins, speaker_pins};
//!
//! let p = esp_hal::init(esp_hal::Config::default());
//! // …start esp-rtos (or another embassy-time driver)…
//! let (mut display, mut backlight) = cardputer_adv::display(display_pins!(p))?;
//! let bus = cardputer_adv::i2c(i2c_pins!(p))?;
//! let mut keyboard = cardputer_adv::keyboard_on(bus, p.GPIO11).await?;
//! let mut imu = cardputer_adv::imu(bus).await?;
//! // In a task of its own: the speaker, then the codec it clocks.
//! let mut speaker = cardputer_adv::speaker(speaker_pins!(p))?;
//! let mut codec = cardputer_adv::codec(bus).await?;
//!
//! for key in keyboard.wait_for_inputs().await? { /* … */ }
//! ```
//!
//! # Features
//!
//! - `sd-card`: the microSD card (`sd_card`), through embedded-sdmmc.
//! - `defmt`: `defmt::Format` for [`Error`].
//!
//! # Pins
//!
//! From the ADV schematic, M5Unified and working firmware:
//!
//! | Function | GPIO |
//! |---|---|
//! | LCD SCK / MOSI / CS / DC / RST | 36 / 35 / 37 / 34 / 33 |
//! | LCD backlight, PWM on LEDC (also powers the RGB LED) | 38 |
//! | I2C SDA / SCL (keyboard, ES8311, BMI270) | 8 / 9 |
//! | Keyboard interrupt | 11 |
//! | Battery voltage (ADC1, through a 2:1 divider) | 10 |
//! | I2S BCLK / WS / DOUT / DIN (the ES8311; no MCLK) | 41 / 43 / 42 / 46 |
//! | G0 button | 0 |
//! | RGB LED (WS2812) | 21 |
//! | IR LED | 44 |
//! | microSD SCK / MOSI / MISO / CS | 40 / 14 / 39 / 12 |

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
#![expect(
    clippy::future_not_send,
    reason = "esp-hal's async drivers aren't Send; embassy polls a future on the executor that \
              made it"
)]

mod audio;
mod battery;
mod bmi270;
mod button;
mod codec;
mod convert;
mod display;
mod error;
mod frame;
mod frame_display;
mod i2c;
mod imu;
mod ir;
mod keyboard;
mod keymap;
pub mod keys;
mod led;
mod microphone;
mod panel;
mod pins;
mod pulses;
mod remote;
mod repeat;
#[cfg(feature = "sd-card")]
mod sd_card;
mod speaker;
mod tca8418;
mod tone;
mod units;

pub use audio::{Audio, AudioPins, audio};
pub use battery::{Battery, BatteryPins, battery};
pub use bmi270::{AccelerometerRange, GyroscopeRange, ImuMode};
pub use button::{Button, Click, button};
pub use codec::{Codec, codec};
pub use convert::battery_percent;
pub use display::{Backlight, Display, DisplayPins, display};
/// The microSD card's FAT file system and driver, at the version
/// [`SdCard`] uses.
#[cfg(feature = "sd-card")]
pub use embedded_sdmmc;
pub use error::Error;
pub use frame::Frame;
pub use frame_display::{FrameDisplay, frame_display};
pub use i2c::{I2cBus, I2cDevice, I2cPins, i2c};
pub use imu::{Imu, imu};
pub use ir::{IrTransmitter, ir_transmitter};
pub use keyboard::{Keyboard, KeyboardPins, keyboard, keyboard_on};
pub use led::{RgbLed, rgb_led};
/// The display driver [`Display`] comes from, so its types can be named
/// without matching its version.
pub use mipidsi;
pub use panel::{HEIGHT, WIDTH};
pub use pulses::{ExtendedNecCode, NecCode, Pulse};
pub use remote::{RMT_FREQUENCY, rmt};
pub use repeat::Repeat;
#[cfg(feature = "sd-card")]
pub use sd_card::{SdCard, SdCardPins, sd_card};
pub use speaker::{Speaker, SpeakerPins, speaker};
pub use tone::{SAMPLE_RATE, Tone, Waveform};
pub use units::{
    Acceleration, Decibels, Hertz, Millicelsius, Millivolts, Motion, Percent, Rotation,
};
