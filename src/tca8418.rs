//! The TCA8418 keyboard controller: setting it up, and reading its key
//! events into what they type, pressed and released. It's generic over the I2C bus, so the host
//! tests can run it against a fake; [`Keyboard`] adds the interrupt line and
//! the waiting.
//!
//! [`Keyboard`]: crate::Keyboard

use core::{array, iter, mem};

use embedded_hal_async::i2c::I2c;

use crate::{
    keymap::Keymap,
    keys::{KeyEvent, KeyInput},
};

/// Its I2C address.
const ADDRESS: u8 = 0x34;

/// Its registers.
const CFG: u8 = 0x01;
const INT_STAT: u8 = 0x02;
/// The number of events in the FIFO, in its low nibble.
const KEY_LCK_EC: u8 = 0x03;
/// The FIFO: reading it takes the oldest event.
const KEY_EVENT_A: u8 = 0x04;
/// The first of three registers (rows, columns 0–7, columns 8–9) setting
/// which pins are the key matrix.
const KP_GPIO1: u8 = 0x1D;

/// `CFG`: the interrupt line goes low on key events.
const KE_IEN: u8 = 1 << 0;
/// `INT_STAT`: every flag, to clear them.
const ALL_INTERRUPTS: u8 = 0x1F;
/// `KP_GPIO1`–`3`: the keyboard's matrix, rows 0–6 and columns 0–7.
const MATRIX: [u8; 3] = [0x7F, 0xFF, 0x00];

/// The most key events one read returns (the FIFO holds 10).
const MAX_EVENTS: usize = 10;

/// Key events taken from [`Tca8418`], in order.
pub type Events = iter::Flatten<array::IntoIter<Option<KeyEvent>, MAX_EVENTS>>;

/// No key events.
pub fn no_events() -> Events {
    [None; MAX_EVENTS].into_iter().flatten()
}

/// Why the controller is being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// Its interrupt line went low.
    Interrupt,
    /// The fallback poll, with no interrupt.
    Fallback,
    /// Events were left from the last read ([`Tca8418::is_pending`]).
    Pending,
}

/// The TCA8418, and the key events read from it and not yet returned.
pub struct Tca8418<I> {
    i2c: I,
    /// Which keys are held.
    keymap: Keymap,
    /// Events read and not yet returned, in order: the first `count`.
    /// They're kept here so that a cancelled read loses none of them.
    events: [Option<KeyEvent>; MAX_EVENTS],
    count: usize,
    /// Events arrived while the last batch was being read: read again
    /// without waiting.
    pending: bool,
    missed: u32,
}

impl<I: I2c> Tca8418<I> {
    /// The controller on `i2c`, not yet set up.
    #[must_use]
    pub const fn new(i2c: I) -> Self {
        Self {
            i2c,
            keymap: Keymap::new(),
            events: [None; MAX_EVENTS],
            count: 0,
            pending: false,
            missed: 0,
        }
    }

    /// Sets it up: the key matrix, an empty FIFO, and the interrupt on key
    /// events.
    pub async fn set_up(&mut self) -> Result<(), I::Error> {
        for (register, pins) in (KP_GPIO1..).zip(MATRIX) {
            self.write(register, pins).await?;
        }
        // Drop what's queued from before.
        for _ in 0..self.event_count().await? {
            self.read(KEY_EVENT_A).await?;
        }
        self.write(INT_STAT, ALL_INTERRUPTS).await?;
        let config = self.read(CFG).await?;
        self.write(CFG, config | KE_IEN).await?;
        self.write(INT_STAT, ALL_INTERRUPTS).await
    }

    /// The events read and not yet returned, if there are any.
    pub fn take_events(&mut self) -> Option<Events> {
        if self.count == 0 {
            return None;
        }
        self.count = 0;
        Some(
            mem::replace(&mut self.events, [None; MAX_EVENTS])
                .into_iter()
                .flatten(),
        )
    }

    /// Reads up to a FIFO's worth of events, woken by `wake`, and returns
    /// how many there were. They wait in [`take_events`](Self::take_events).
    ///
    /// Cancelling it, or a failure, loses no event already read. A cancel in
    /// the middle of the I2C transfer that takes an event from the FIFO can
    /// lose that one.
    pub async fn read_events(&mut self, wake: Wake) -> Result<u8, I::Error> {
        // At most a FIFO's worth: any more stay in it, and keep the line
        // low, for next time.
        let count = self.event_count().await?;
        for _ in 0..usize::from(count).min(MAX_EVENTS) {
            let event = self.read(KEY_EVENT_A).await?;
            // Numbers that aren't keys are dropped.
            if let Some(event) = self.keymap.event(event) {
                self.events[self.count] = Some(event);
                self.count += 1;
            }
        }
        // Clear every flag (key events, overflow, …) so no source keeps the
        // line low, then catch an event that landed in between. The keys are
        // already read, so a failure here mustn't lose them: read again
        // next time instead. Until then, `pending` stays as it was, so a
        // cancelled read doesn't lose it either.
        let cleared = self.write(INT_STAT, ALL_INTERRUPTS).await.is_ok();
        self.pending = !cleared || !self.event_count().await.is_ok_and(|count| count == 0);

        if count > 0 && wake == Wake::Fallback {
            // Events without an interrupt: the line isn't working.
            self.missed = self.missed.saturating_add(1);
        }
        Ok(count)
    }

    /// Whether a key that types `input`, with or without Shift or Fn, is
    /// held, as of the last event read.
    #[must_use]
    pub fn is_held(&self, input: KeyInput) -> bool {
        self.keymap.is_held(input)
    }

    /// Whether events arrived while the last batch was being read, to be
    /// read without waiting.
    pub const fn is_pending(&self) -> bool {
        self.pending
    }

    /// How many times events were found by the fallback poll, with no
    /// interrupt.
    pub const fn missed_interrupts(&self) -> u32 {
        self.missed
    }

    /// How many events the FIFO holds.
    async fn event_count(&mut self) -> Result<u8, I::Error> {
        Ok(self.read(KEY_LCK_EC).await? & 0x0F)
    }

    async fn read(&mut self, register: u8) -> Result<u8, I::Error> {
        let mut value = [0];
        self.i2c
            .write_read(ADDRESS, &[register], &mut value)
            .await?;
        Ok(value[0])
    }

    async fn write(&mut self, register: u8, value: u8) -> Result<(), I::Error> {
        self.i2c.write(ADDRESS, &[register, value]).await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        pin::pin,
        task::{Context, Waker},
    };

    use embassy_futures::{block_on, yield_now};
    use embedded_hal_async::i2c::{ErrorKind, ErrorType, Operation};

    use super::*;
    use crate::keys::Modifier;

    /// A TCA8418: its registers, and its FIFO behind `KEY_LCK_EC` and
    /// `KEY_EVENT_A`.
    #[derive(Default)]
    struct Fake {
        registers: [u8; 0x20],
        fifo: VecDeque<u8>,
        /// The register a read reads.
        register: u8,
        /// Events taken before the next one waits once (so a read of them
        /// can be cancelled there), or fails.
        pause_after: Option<usize>,
        fail_after: Option<usize>,
        /// A register whose writes fail.
        failing_write: Option<u8>,
    }

    impl Fake {
        fn with_events(events: &[u8]) -> Self {
            Self {
                fifo: events.iter().copied().collect(),
                ..Self::default()
            }
        }

        /// Takes the next event, when a read of `KEY_EVENT_A` gets to it.
        async fn take_event(&mut self) -> Result<u8, ErrorKind> {
            for countdown in [&mut self.pause_after, &mut self.fail_after] {
                *countdown = countdown.map(|left| left.wrapping_sub(1));
            }
            if self.pause_after == Some(usize::MAX) {
                yield_now().await;
            }
            if self.fail_after == Some(usize::MAX) {
                return Err(ErrorKind::Other);
            }
            Ok(self.fifo.pop_front().unwrap_or(0))
        }
    }

    impl ErrorType for Fake {
        type Error = ErrorKind;
    }

    impl I2c for Fake {
        async fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> Result<(), ErrorKind> {
            assert_eq!(address, ADDRESS);
            for operation in operations {
                match operation {
                    Operation::Write([register]) => self.register = *register,
                    Operation::Write([register, value]) => {
                        if self.failing_write == Some(*register) {
                            return Err(ErrorKind::Other);
                        }
                        self.registers[usize::from(*register)] = *value;
                        if *register == INT_STAT {
                            self.registers[usize::from(INT_STAT)] = 0;
                        }
                    }
                    Operation::Write(_) => unreachable!("one register at a time"),
                    Operation::Read(buffer) => {
                        buffer[0] = match self.register {
                            KEY_LCK_EC => u8::try_from(self.fifo.len().min(10)).unwrap(),
                            KEY_EVENT_A => self.take_event().await?,
                            register => self.registers[usize::from(register)],
                        };
                    }
                }
            }
            Ok(())
        }
    }

    const fn press(number: u8) -> u8 {
        number | 0x80
    }

    /// Key numbers (matrix row × 10 + column + 1).
    const SHIFT: u8 = 7;
    const A: u8 = 13;
    const D: u8 = 23;

    /// What the presses taken type.
    fn taken(controller: &mut Tca8418<Fake>) -> Vec<KeyInput> {
        controller
            .take_events()
            .into_iter()
            .flatten()
            .filter(|event| event.is_press())
            .map(KeyEvent::input)
            .collect()
    }

    #[test]
    fn setting_up_empties_the_fifo_and_turns_the_interrupt_on() {
        let mut controller = Tca8418::new(Fake::with_events(&[press(A), A]));
        block_on(controller.set_up()).unwrap();
        assert!(controller.i2c.fifo.is_empty());
        assert_eq!(controller.i2c.registers[usize::from(CFG)], KE_IEN);
        assert_eq!(controller.i2c.registers[0x1D..0x20], MATRIX);
        assert_eq!(controller.take_events().map(Iterator::count), None);
    }

    #[test]
    fn presses_and_releases_come_in_order() {
        let events = [press(A), A, press(SHIFT), press(A), A, SHIFT];
        let mut controller = Tca8418::new(Fake::with_events(&events));
        assert_eq!(block_on(controller.read_events(Wake::Interrupt)), Ok(6));
        let shift = KeyInput::Modifier(Modifier::Shift);
        assert_eq!(
            controller
                .take_events()
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            [
                KeyEvent::Pressed(KeyInput::Char('a')),
                KeyEvent::Released(KeyInput::Char('a')),
                KeyEvent::Pressed(shift),
                KeyEvent::Pressed(KeyInput::Char('A')),
                KeyEvent::Released(KeyInput::Char('A')),
                KeyEvent::Released(shift),
            ]
        );
        assert!(!controller.is_pending());
        // Each taken once.
        assert_eq!(controller.take_events().map(Iterator::count), None);
    }

    #[test]
    fn events_left_in_the_fifo_are_pending() {
        let events = [press(A), A].repeat(6);
        let mut controller = Tca8418::new(Fake::with_events(&events));
        assert_eq!(block_on(controller.read_events(Wake::Interrupt)), Ok(10));
        assert_eq!(controller.take_events().map(Iterator::count), Some(10));
        assert!(controller.is_pending());
        assert_eq!(block_on(controller.read_events(Wake::Pending)), Ok(2));
        assert_eq!(controller.take_events().map(Iterator::count), Some(2));
        assert!(!controller.is_pending());
    }

    #[test]
    fn a_cancelled_read_keeps_the_keys_it_read() {
        let fake = Fake {
            pause_after: Some(2),
            ..Fake::with_events(&[press(A), press(D), press(A)])
        };
        let mut controller = Tca8418::new(fake);
        controller.pending = true;
        {
            let mut read = pin!(controller.read_events(Wake::Pending));
            let mut context = Context::from_waker(Waker::noop());
            assert!(read.as_mut().poll(&mut context).is_pending());
            // Dropped here: cancelled before the third event.
        }
        assert_eq!(
            taken(&mut controller),
            [KeyInput::Char('a'), KeyInput::Char('d')]
        );
        assert!(controller.is_pending(), "still to be read");
        assert_eq!(block_on(controller.read_events(Wake::Pending)), Ok(1));
        assert_eq!(taken(&mut controller), [KeyInput::Char('a')]);
    }

    #[test]
    fn a_failed_read_keeps_the_keys_before_it() {
        let fake = Fake {
            fail_after: Some(1),
            ..Fake::with_events(&[press(A), press(D)])
        };
        let mut controller = Tca8418::new(fake);
        assert_eq!(
            block_on(controller.read_events(Wake::Interrupt)),
            Err(ErrorKind::Other)
        );
        assert_eq!(taken(&mut controller), [KeyInput::Char('a')]);
    }

    #[test]
    fn a_failed_clear_reads_again_without_losing_keys() {
        let fake = Fake {
            failing_write: Some(INT_STAT),
            ..Fake::with_events(&[press(A)])
        };
        let mut controller = Tca8418::new(fake);
        assert_eq!(block_on(controller.read_events(Wake::Interrupt)), Ok(1));
        assert_eq!(taken(&mut controller), [KeyInput::Char('a')]);
        assert!(controller.is_pending());
    }

    #[test]
    fn only_events_the_fallback_poll_finds_are_missed_interrupts() {
        let mut controller = Tca8418::new(Fake::default());
        block_on(controller.read_events(Wake::Fallback)).unwrap();
        controller.i2c.fifo.push_back(press(A));
        block_on(controller.read_events(Wake::Interrupt)).unwrap();
        assert_eq!(controller.missed_interrupts(), 0);
        controller.i2c.fifo.push_back(press(A));
        block_on(controller.read_events(Wake::Fallback)).unwrap();
        assert_eq!(controller.missed_interrupts(), 1);
    }
}
