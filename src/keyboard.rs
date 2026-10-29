//! The keyboard: a TCA8418 on the I2C bus, and its interrupt line.

use core::fmt;

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use esp_hal::{
    gpio::{Event, Input, InputConfig, Pull},
    peripherals::GPIO11,
};

use crate::{
    Error, I2cBus, I2cDevice, I2cPins, i2c,
    keys::KeyInput,
    tca8418::{Tca8418, Wake},
};

/// The peripherals the keyboard uses.
#[derive(Debug)]
pub struct KeyboardPins {
    /// The I2C bus it shares (set up by [`keyboard`]).
    pub i2c: I2cPins,
    /// The TCA8418's interrupt output (active low).
    pub interrupt: GPIO11<'static>,
}

/// How often to look for key events anyway, in case an interrupt is missed
/// (or the line doesn't work): key presses are then late, not lost.
const FALLBACK_POLL: Duration = Duration::from_secs(1);

/// The pause after a wake-up with nothing to read. If the line is stuck low
/// this turns a busy loop into slow polling, so other tasks keep running.
const SPURIOUS_BACKOFF: Duration = Duration::from_millis(10);

/// The keyboard: the TCA8418 and its interrupt line.
pub struct Keyboard {
    controller: Tca8418<I2cDevice>,
    interrupt: Input<'static>,
}

impl Keyboard {
    /// Waits for key presses and returns them (with Shift/Fn applied).
    ///
    /// Sleeps until the TCA8418 pulls its interrupt line low, so an idle
    /// keyboard costs nothing. It waits for the low *level*, not an edge:
    /// events that arrive while the caller is busy keep the line low.
    ///
    /// Cancelling it (in a `select`, say) loses no key that's been read: the
    /// next call returns it. The one exception is a cancel in the middle of
    /// an I2C transfer that's taking an event from the TCA8418, which can
    /// lose that one.
    ///
    /// # Errors
    ///
    /// If the TCA8418 can't be read over I2C.
    pub async fn wait_for_inputs(
        &mut self,
    ) -> Result<impl Iterator<Item = KeyInput> + use<>, Error> {
        loop {
            // Keys read by a call that was cancelled: return them before
            // anything else, which could fail.
            if let Some(inputs) = self.controller.take_inputs() {
                return Ok(inputs);
            }
            let wake = if self.controller.is_pending() {
                Wake::Pending
            } else {
                match select(self.interrupt.wait_for_low(), Timer::after(FALLBACK_POLL)).await {
                    Either::First(()) => Wake::Interrupt,
                    Either::Second(()) => Wake::Fallback,
                }
            };
            let count = self
                .controller
                .read_events(wake)
                .await
                .map_err(|_| Error::Keyboard)?;
            if count == 0 && wake == Wake::Interrupt {
                // Woken with nothing to read: back off, so a line stuck low
                // can't keep this task from yielding.
                Timer::after(SPURIOUS_BACKOFF).await;
            }
        }
    }

    /// Makes a key press end a light sleep (esp-hal's
    /// `LowPower::sleep_light`): the interrupt line listens for its low
    /// level, which wakes the chip. Call it just before sleeping; the key is
    /// then read by [`wait_for_inputs`](Self::wait_for_inputs) as usual.
    pub fn listen_for_wake(&mut self) {
        self.interrupt.listen(Event::LowLevel);
    }

    /// Whether a key that types `input` is held down, with or without
    /// Shift and Fn: `Char('a')` and `Char('A')` are both the A key, and
    /// `Arrow(Arrow::Up)` is the `;` key. For games, and for chords such as
    /// Ctrl + C (`Modifier(Modifier::Ctrl)` held when `c` comes).
    ///
    /// It's as of the last key event read. [`wait_for_inputs`] reads
    /// events, releases among them, as they come while it's waiting, so
    /// keep it waiting (in a `select` with a frame timer, say) for this to
    /// stay current.
    ///
    /// [`wait_for_inputs`]: Self::wait_for_inputs
    #[must_use]
    pub fn is_held(&self, input: KeyInput) -> bool {
        self.controller.is_held(input)
    }

    /// How many times key events were found only by the fallback poll, which
    /// points at a broken interrupt line.
    #[must_use]
    pub const fn missed_interrupts(&self) -> u32 {
        self.controller.missed_interrupts()
    }
}

/// Initialises the I2C bus, the keyboard and its interrupt. To use the
/// motion sensor too, set the bus up with [`i2c`] and use
/// [`keyboard_on`] instead.
///
/// # Errors
///
/// If the bus or the TCA8418 can't be set up.
///
/// # Panics
///
/// If the bus was already set up (by an earlier call, or [`i2c`]).
pub async fn keyboard(pins: KeyboardPins) -> Result<Keyboard, Error> {
    let bus = i2c(pins.i2c)?;
    keyboard_on(bus, pins.interrupt).await
}

/// Tries at setting up the TCA8418: right after a flash or a reset it can
/// miss the first transfers.
const KEYBOARD_TRIES: u32 = 5;
/// The wait between those tries.
const KEYBOARD_RETRY: Duration = Duration::from_millis(20);

/// Initialises the keyboard on the shared bus, and its interrupt.
///
/// # Errors
///
/// If the TCA8418 can't be set up, after five tries (it can miss the first
/// transfers right after a flash or a reset).
pub async fn keyboard_on(
    bus: &'static I2cBus,
    interrupt: GPIO11<'static>,
) -> Result<Keyboard, Error> {
    let mut keyboard = Keyboard {
        controller: Tca8418::new(I2cDevice::new(bus)),
        // The TCA8418's INT is open-drain.
        interrupt: Input::new(interrupt, InputConfig::default().with_pull(Pull::Up)),
    };
    for attempt in 0..KEYBOARD_TRIES {
        if attempt > 0 {
            Timer::after(KEYBOARD_RETRY).await;
        }
        if keyboard.controller.set_up().await.is_ok() {
            return Ok(keyboard);
        }
    }
    Err(Error::Keyboard)
}

impl fmt::Debug for Keyboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keyboard")
            .field("pending", &self.controller.is_pending())
            .field("missed_interrupts", &self.missed_interrupts())
            .finish_non_exhaustive()
    }
}
