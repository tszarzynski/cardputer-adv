//! The keyboard: a TCA8418 on the I2C bus, and its interrupt line.

use core::{fmt, mem};

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use embedded_hal_bus::i2c::CriticalSectionDevice;
use esp_hal::{
    delay::Delay,
    gpio::{Event, Input, InputConfig, Pull},
    peripherals::GPIO11,
};

use crate::{Error, I2cBus, I2cDevice, I2cPins, i2c, keys};

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

/// The most key inputs one read returns (the TCA8418's FIFO holds 10
/// events).
const MAX_INPUTS: usize = 10;

/// The keyboard: the TCA8418 driver and its interrupt line.
pub struct Keyboard {
    keys: keys::Keyboard<I2cDevice>,
    interrupt: Input<'static>,
    /// Events arrived while the last batch was being read: read again
    /// without waiting.
    pending: bool,
    missed: u32,
}

/// Why [`Keyboard::wait_for_inputs`] went to read the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    Interrupt,
    Fallback,
    Pending,
}

impl Keyboard {
    /// Waits for key presses and returns them (with Shift/Fn applied).
    ///
    /// Sleeps until the TCA8418 pulls its interrupt line low, so an idle
    /// keyboard costs nothing. It waits for the low *level*, not an edge:
    /// events that arrive while the caller is busy keep the line low.
    ///
    /// # Errors
    ///
    /// If the TCA8418 can't be read over I2C.
    pub async fn wait_for_inputs(&mut self) -> Result<impl Iterator<Item = keys::KeyInput>, Error> {
        loop {
            let wake = if mem::take(&mut self.pending) {
                Wake::Pending
            } else {
                match select(self.interrupt.wait_for_low(), Timer::after(FALLBACK_POLL)).await {
                    Either::First(()) => Wake::Interrupt,
                    Either::Second(()) => Wake::Fallback,
                }
            };
            let events = self.keys.events().map_err(|_| Error::Keyboard)?;
            // Clear every flag (key events, overflow, …) so no source keeps
            // the line low, then catch an event that landed in between. The
            // keys are already read, so a failure here mustn't lose them:
            // read again on the next call instead.
            let cleared = self.keys.clear_interrupts().is_ok();
            self.pending = !cleared || self.keys.events_available().unwrap_or(true);

            let mut events = events.peekable();
            let any_event = events.peek().is_some();
            let mut inputs = [None; MAX_INPUTS];
            for (slot, input) in inputs
                .iter_mut()
                .zip(events.filter_map(|event| event.input))
            {
                *slot = Some(input);
            }
            if !any_event {
                // Woken with nothing to read: back off, so a line stuck low
                // can't keep this task from yielding.
                if wake == Wake::Interrupt {
                    Timer::after(SPURIOUS_BACKOFF).await;
                }
                continue;
            }
            if wake == Wake::Fallback {
                // Events without an interrupt: the line isn't working.
                self.missed = self.missed.saturating_add(1);
            }
            // Releases (and lone modifiers) are events without an input.
            if inputs[0].is_none() {
                continue;
            }
            return Ok(inputs.into_iter().flatten());
        }
    }

    /// Makes a key press end a light sleep (esp-hal's
    /// `LowPower::sleep_light`): the interrupt line listens for its low
    /// level, which wakes the chip. Call it just before sleeping; the key is
    /// then read by [`wait_for_inputs`](Self::wait_for_inputs) as usual.
    pub fn listen_for_wake(&mut self) {
        self.interrupt.listen(Event::LowLevel);
    }

    /// How many times key events were found only by the fallback poll, which
    /// points at a broken interrupt line.
    #[must_use]
    pub const fn missed_interrupts(&self) -> u32 {
        self.missed
    }

    /// The underlying driver, for anything [`wait_for_inputs`](Self::wait_for_inputs)
    /// doesn't cover.
    pub const fn driver_mut(&mut self) -> &mut keys::Keyboard<I2cDevice> {
        &mut self.keys
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
pub fn keyboard(pins: KeyboardPins) -> Result<Keyboard, Error> {
    let bus = i2c(pins.i2c)?;
    keyboard_on(bus, pins.interrupt)
}

/// Tries at setting up the TCA8418: right after a flash or a reset it can
/// miss the first transfers.
const KEYBOARD_TRIES: u32 = 5;
/// The wait between those tries.
const KEYBOARD_RETRY_MS: u32 = 20;

/// Initialises the keyboard on the shared bus, and its interrupt.
///
/// # Errors
///
/// If the TCA8418 can't be set up, after five tries (it can miss the first
/// transfers right after a flash or a reset).
pub fn keyboard_on(bus: &'static I2cBus, interrupt: GPIO11<'static>) -> Result<Keyboard, Error> {
    let set_up = || -> Result<keys::Keyboard<I2cDevice>, Error> {
        let mut keys =
            keys::Keyboard::new(CriticalSectionDevice::new(bus)).map_err(|_| Error::Keyboard)?;
        keys.enable_keyboard_interrupt()
            .map_err(|_| Error::Keyboard)?;
        keys.clear_interrupts().map_err(|_| Error::Keyboard)?;
        Ok(keys)
    };
    let keys = (0..KEYBOARD_TRIES)
        .find_map(|attempt| {
            if attempt > 0 {
                Delay::new().delay_millis(KEYBOARD_RETRY_MS);
            }
            set_up().ok()
        })
        .ok_or(Error::Keyboard)?;
    // The TCA8418's INT is open-drain.
    let interrupt = Input::new(interrupt, InputConfig::default().with_pull(Pull::Up));
    Ok(Keyboard {
        keys,
        interrupt,
        pending: false,
        missed: 0,
    })
}

impl fmt::Debug for Keyboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keyboard")
            .field("pending", &self.pending)
            .field("missed_interrupts", &self.missed)
            .finish_non_exhaustive()
    }
}
