//! The keyboard: a TCA8418 on the I2C bus, and its interrupt line.

use core::{fmt, time};

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    gpio::{Event, Input, InputConfig, Pull},
    peripherals::GPIO11,
};

use crate::{
    Error, I2cBus, I2cDevice, I2cPins, i2c,
    keys::{KeyEvent, KeyInput},
    repeat::{Repeat, Repeater},
    tca8418::{Events, Tca8418, Wake, no_events},
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
    repeater: Repeater,
}

/// The time since boot, as the repeater keeps it.
fn now() -> time::Duration {
    time::Duration::from_micros(Instant::now().as_micros())
}

impl Keyboard {
    /// Waits for key presses and returns them (with Shift/Fn applied), and
    /// a held key's repeats if [`set_repeat`](Self::set_repeat) turned them
    /// on. Releases are dropped: [`wait_for_events`](Self::wait_for_events)
    /// returns those too.
    ///
    /// Waits as [`wait_for_events`](Self::wait_for_events) does, and can be
    /// cancelled the same way.
    ///
    /// # Errors
    ///
    /// If the TCA8418 can't be read over I2C.
    pub async fn wait_for_inputs(
        &mut self,
    ) -> Result<impl Iterator<Item = KeyInput> + use<>, Error> {
        loop {
            // With no key repeating, the timer never fires.
            let at = self.repeater.next_at().map_or(Instant::MAX, |at| {
                Instant::from_micros(u64::try_from(at.as_micros()).unwrap_or(u64::MAX))
            });
            let (events, repeated) = match select(self.events(), Timer::at(at)).await {
                Either::First(events) => (events?, None),
                Either::Second(()) => (no_events(), self.repeater.due(now())),
            };
            let mut inputs = events
                .filter(|event| event.is_press())
                .map(KeyEvent::input)
                .chain(repeated)
                .peekable();
            if inputs.peek().is_some() {
                return Ok(inputs);
            }
        }
    }

    /// Turns auto-repeat on, as `repeat`, or off (`None`, as
    /// [`keyboard`] leaves it): a key held past the delay is typed again
    /// every interval, by [`wait_for_inputs`](Self::wait_for_inputs), as on
    /// a PC. For text and menus; [`Repeat::default`] is about a PC's.
    ///
    /// The last key pressed repeats, until it's released or another is
    /// pressed; modifiers don't. [`wait_for_events`](Self::wait_for_events)
    /// returns only the keys' own presses and releases, for games.
    pub const fn set_repeat(&mut self, repeat: Option<Repeat>) {
        self.repeater.set(repeat);
    }

    /// Waits for keys going down or coming up, and returns them in order:
    /// for games, where letting go of a key matters as much as pressing it.
    /// A release says what its press typed.
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
    pub async fn wait_for_events(
        &mut self,
    ) -> Result<impl Iterator<Item = KeyEvent> + use<>, Error> {
        self.events().await
    }

    /// [`wait_for_events`](Self::wait_for_events), as the type it is, and
    /// passing the events to the repeater.
    async fn events(&mut self) -> Result<Events, Error> {
        loop {
            // Events read by a call that was cancelled: return them before
            // anything else, which could fail.
            if let Some(events) = self.controller.take_events() {
                let now = now();
                for event in events.clone() {
                    self.repeater.event(event, now);
                }
                return Ok(events);
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
    /// It's as of the last key event read. [`wait_for_inputs`] and
    /// [`wait_for_events`] read events, releases among them, as they come
    /// while they're waiting, so keep one waiting (in a `select` with a
    /// frame timer, say) for this to stay current.
    ///
    /// [`wait_for_inputs`]: Self::wait_for_inputs
    /// [`wait_for_events`]: Self::wait_for_events
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
        repeater: Repeater::new(),
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
