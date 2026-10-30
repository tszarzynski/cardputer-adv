//! The whole board in one call: [`board!`](crate::board!) sets up every part that has
//! nothing to choose, and hands over the pins of those that do.

use core::fmt;

use esp_hal::peripherals::{GPIO0, GPIO21, GPIO44, RMT};

use crate::{
    AudioPins, Battery, BatteryPins, Button, DisplayPins, Error, I2cBus, IrTransmitter, Keyboard,
    KeyboardPins, RgbLed, battery, button, i2c, ir_transmitter, keyboard_on, rgb_led, rmt,
};

/// The peripherals [`Board::new`] takes, from [`board_pins!`](crate::board_pins!).
#[expect(missing_docs, reason = "each field is the part it's named after")]
pub struct BoardPins {
    pub display: DisplayPins,
    pub keyboard: KeyboardPins,
    pub audio: AudioPins,
    pub battery: BatteryPins,
    pub button: GPIO0<'static>,
    pub rmt: RMT<'static>,
    pub led: GPIO21<'static>,
    pub ir: GPIO44<'static>,
}

/// The board, from [`board!`](crate::board!): the parts set up and ready, and the pins of
/// the two that need a choice.
///
/// ```ignore
/// let Board { display, keyboard, bus, .. } = cardputer_adv::board!(p).await?;
/// let (mut display, mut backlight) = cardputer_adv::display(display)?;
/// let mut imu = cardputer_adv::imu(bus).await?;
/// ```
///
/// - The display: [`display`](crate::display) draws straight to the
///   screen, and [`frame_display`](crate::frame_display) from a
///   [`Frame`](crate::Frame) by DMA.
/// - The audio: [`speaker`](crate::speaker) alone, or
///   [`audio`](crate::audio) with the microphone, usually started in a
///   task of its own, before [`codec`](crate::codec).
/// - The motion sensor isn't started: [`imu`](crate::imu) on [`bus`]
///   takes about 230 ms and draws current, so it's up to the app.
///
/// The microSD card stays out (`sd_card_pins!`, with the `sd-card`
/// feature), as do the RMT's other two channels.
///
/// [`bus`]: Self::bus
#[non_exhaustive]
pub struct Board {
    /// The LCD's pins, for [`display`](crate::display) or
    /// [`frame_display`](crate::frame_display).
    pub display: DisplayPins,
    /// The speaker's and the microphone's pins, for
    /// [`speaker`](crate::speaker) (with `audio.speaker`) or
    /// [`audio`](crate::audio).
    pub audio: AudioPins,
    /// The I2C bus the keyboard, the codec and the motion sensor share, for
    /// [`imu`](crate::imu) and [`codec`](crate::codec).
    pub bus: &'static I2cBus,
    /// The keyboard.
    pub keyboard: Keyboard,
    /// The G0 button.
    pub button: Button,
    /// The battery's voltage and charge.
    pub battery: Battery,
    /// The RGB LED, on the RMT's channel 0.
    pub led: RgbLed,
    /// The IR transmitter, on the RMT's channel 1.
    pub ir: IrTransmitter,
}

impl Board {
    /// Sets up every part with nothing to choose: the I2C bus, the
    /// keyboard, G0, the battery measurement, and the RMT with the RGB LED
    /// and the IR transmitter. [`board!`](crate::board!) gives it the pins.
    ///
    /// # Errors
    ///
    /// If a part can't be set up: the bus, the keyboard (after its retries)
    /// or the RMT.
    ///
    /// # Panics
    ///
    /// If the bus was set up already ([`i2c`] or [`keyboard`]): it's
    /// static.
    ///
    /// [`keyboard`]: crate::keyboard
    pub async fn new(pins: BoardPins) -> Result<Self, Error> {
        let bus = i2c(pins.keyboard.i2c)?;
        let keyboard = keyboard_on(bus, pins.keyboard.interrupt).await?;
        let rmt = rmt(pins.rmt)?;
        Ok(Self {
            display: pins.display,
            audio: pins.audio,
            bus,
            keyboard,
            button: button(pins.button),
            battery: battery(pins.battery),
            led: rgb_led(rmt.channel0, pins.led)?,
            ir: ir_transmitter(rmt.channel1, pins.ir)?,
        })
    }
}

impl fmt::Debug for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Board")
            .field("keyboard", &self.keyboard)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for BoardPins {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoardPins").finish_non_exhaustive()
    }
}
