//! The G0 button.

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use esp_hal::{
    gpio::{Event, Input, InputConfig, Pull},
    peripherals::GPIO0,
};

/// How long the G0 button has to stay down to count as pressed: its contacts
/// bounce for a few milliseconds.
const DEBOUNCE: Duration = Duration::from_millis(10);
/// How long a press lasts to be a [`Click::Long`].
const LONG_PRESS: Duration = Duration::from_millis(500);
/// How soon after a click the next press makes it a [`Click::Double`].
const DOUBLE_CLICK_GAP: Duration = Duration::from_millis(300);

/// What a press of the button was, as [`Button::wait_for_click`] tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum Click {
    /// Pressed and released, with no second press soon after.
    Single,
    /// Two presses in quick succession (300 ms apart at most).
    Double,
    /// Held down for half a second.
    Long,
}

/// The G0 button. Held while the Cardputer starts, it enters the ROM
/// bootloader; after that it's free for the app.
#[derive(Debug)]
pub struct Button {
    input: Input<'static>,
}

impl Button {
    /// Whether it's held down now.
    #[must_use]
    pub fn is_pressed(&self) -> bool {
        self.input.is_low()
    }

    /// Waits for a press: the button going down (and staying down past its
    /// bounce) after being up. If it's held down when this is called, that
    /// press doesn't count.
    pub async fn wait_for_press(&mut self) {
        loop {
            self.input.wait_for_high().await;
            self.input.wait_for_low().await;
            Timer::after(DEBOUNCE).await;
            if self.is_pressed() {
                return;
            }
        }
    }

    /// Waits until it's up, past its bounce: at once if it's up already.
    pub async fn wait_for_release(&mut self) {
        loop {
            self.input.wait_for_high().await;
            Timer::after(DEBOUNCE).await;
            if !self.is_pressed() {
                return;
            }
        }
    }

    /// Waits for a press and tells what it was: a [single](Click::Single)
    /// or [double](Click::Double) click, or a [long](Click::Long) press.
    /// With one button, these give it three jobs.
    ///
    /// A long press is told once it's lasted half a second, while it's
    /// still held, so the app can react then; [`wait_for_release`] waits
    /// for its end. A single click is told 300 ms after its release, once
    /// no second press has come: what telling it from a double click
    /// costs. A double click is told at its second press. As with
    /// [`wait_for_press`], a press under way when this is called doesn't
    /// count. Cancelling it drops the click under way.
    ///
    /// [`wait_for_release`]: Self::wait_for_release
    /// [`wait_for_press`]: Self::wait_for_press
    pub async fn wait_for_click(&mut self) -> Click {
        self.wait_for_press().await;
        let held = select(self.wait_for_release(), Timer::after(LONG_PRESS)).await;
        if matches!(held, Either::Second(())) {
            return Click::Long;
        }
        match select(self.wait_for_press(), Timer::after(DOUBLE_CLICK_GAP)).await {
            Either::First(()) => Click::Double,
            Either::Second(()) => Click::Single,
        }
    }

    /// Makes a press end a light sleep (esp-hal's `LowPower::sleep_light`),
    /// as [`Keyboard::listen_for_wake`] does for keys. Call it just before
    /// sleeping.
    ///
    /// [`Keyboard::listen_for_wake`]: crate::Keyboard::listen_for_wake
    pub fn listen_for_wake(&mut self) {
        self.input.listen(Event::LowLevel);
    }
}

/// Sets up the G0 button (GPIO0, active low).
#[must_use]
pub fn button(pin: GPIO0<'static>) -> Button {
    Button {
        input: Input::new(pin, InputConfig::default().with_pull(Pull::Up)),
    }
}
