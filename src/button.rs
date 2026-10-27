//! The G0 button.

use embassy_time::{Duration, Timer};
use esp_hal::{
    gpio::{Event, Input, InputConfig, Pull},
    peripherals::GPIO0,
};

/// How long the G0 button has to stay down to count as pressed: its contacts
/// bounce for a few milliseconds.
const DEBOUNCE: Duration = Duration::from_millis(10);

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
