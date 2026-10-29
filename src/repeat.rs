//! Key auto-repeat's timing: which held key repeats, and when. The
//! [`Keyboard`] feeds it the key events and the time, and waits for it.
//!
//! [`Keyboard`]: crate::Keyboard

use core::time::Duration;

use crate::keys::{KeyEvent, KeyInput};

/// How a held key repeats, for [`Keyboard::set_repeat`]: first after
/// `delay`, then every `interval`.
///
/// [`Keyboard::set_repeat`]: crate::Keyboard::set_repeat
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Repeat {
    /// How long a key is held before it repeats.
    pub delay: Duration,
    /// How often it repeats after that.
    pub interval: Duration,
}

impl Repeat {
    /// A repeat after `delay`, then every `interval`; `core`'s durations or
    /// embassy-time's.
    #[must_use]
    pub fn new(delay: impl Into<Duration>, interval: impl Into<Duration>) -> Self {
        Self {
            delay: delay.into(),
            interval: interval.into(),
        }
    }
}

/// Repeats after 400 ms, then 20 times a second: about a PC's.
impl Default for Repeat {
    fn default() -> Self {
        Self::new(Duration::from_millis(400), Duration::from_millis(50))
    }
}

/// The key repeating, if any, and when it next repeats. The last key
/// pressed repeats, until it's released or another is pressed; modifiers
/// don't repeat, and pressing one doesn't stop a repeat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Repeater {
    repeat: Option<Repeat>,
    /// The key held, and when it next repeats.
    held: Option<(KeyInput, Duration)>,
}

impl Repeater {
    /// Repeating off.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            repeat: None,
            held: None,
        }
    }

    /// Turns repeating on, as `repeat`, or off. A key held now doesn't
    /// repeat until it's pressed again.
    pub const fn set(&mut self, repeat: Option<Repeat>) {
        self.repeat = repeat;
        self.held = None;
    }

    /// Follows a key event that happened at `now` (any steady clock).
    pub fn event(&mut self, event: KeyEvent, now: Duration) {
        let Some(repeat) = self.repeat else {
            return;
        };
        match event {
            KeyEvent::Pressed(KeyInput::Modifier(_)) => {}
            KeyEvent::Pressed(input) => self.held = Some((input, now + repeat.delay)),
            KeyEvent::Released(input) if self.held.is_some_and(|(held, _)| held == input) => {
                self.held = None;
            }
            _ => {}
        }
    }

    /// When the held key next repeats, if one is held.
    #[must_use]
    pub fn next_at(&self) -> Option<Duration> {
        self.held.map(|(_, at)| at)
    }

    /// The held key, if it's due to repeat at `now`. A caller that comes
    /// late gets one repeat, not a burst of the ones it missed.
    pub fn due(&mut self, now: Duration) -> Option<KeyInput> {
        let (input, at) = self.held?;
        let repeat = self.repeat?;
        if now < at {
            return None;
        }
        self.held = Some((input, now + repeat.interval));
        Some(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Modifier;

    const A: KeyInput = KeyInput::Char('a');
    const B: KeyInput = KeyInput::Char('b');
    const SHIFT: KeyInput = KeyInput::Modifier(Modifier::Shift);

    const fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn repeating() -> Repeater {
        let mut repeater = Repeater::new();
        repeater.set(Some(Repeat::new(ms(400), ms(50))));
        repeater
    }

    #[test]
    fn a_held_key_repeats_after_the_delay_then_every_interval() {
        let mut repeater = repeating();
        repeater.event(KeyEvent::Pressed(A), ms(1000));
        assert_eq!(repeater.next_at(), Some(ms(1400)));
        assert_eq!(repeater.due(ms(1399)), None);
        assert_eq!(repeater.due(ms(1400)), Some(A));
        assert_eq!(repeater.due(ms(1449)), None);
        assert_eq!(repeater.due(ms(1450)), Some(A));
        assert_eq!(repeater.next_at(), Some(ms(1500)));
    }

    #[test]
    fn its_release_stops_it_and_another_keys_doesnt() {
        let mut repeater = repeating();
        repeater.event(KeyEvent::Pressed(A), ms(0));
        repeater.event(KeyEvent::Released(B), ms(10));
        assert_eq!(repeater.next_at(), Some(ms(400)));
        repeater.event(KeyEvent::Released(A), ms(20));
        assert_eq!(repeater.next_at(), None);
        assert_eq!(repeater.due(ms(1000)), None);
    }

    #[test]
    fn the_last_key_pressed_repeats_and_modifiers_dont() {
        let mut repeater = repeating();
        repeater.event(KeyEvent::Pressed(A), ms(0));
        repeater.event(KeyEvent::Pressed(B), ms(100));
        repeater.event(KeyEvent::Pressed(SHIFT), ms(200));
        assert_eq!(repeater.due(ms(500)), Some(B));
        // Shift alone never repeats.
        let mut repeater = repeating();
        repeater.event(KeyEvent::Pressed(SHIFT), ms(0));
        assert_eq!(repeater.next_at(), None);
    }

    #[test]
    fn a_late_caller_gets_one_repeat_not_a_burst() {
        let mut repeater = repeating();
        repeater.event(KeyEvent::Pressed(A), ms(0));
        assert_eq!(repeater.due(ms(1000)), Some(A));
        assert_eq!(repeater.due(ms(1000)), None);
        assert_eq!(repeater.next_at(), Some(ms(1050)));
    }

    #[test]
    fn off_nothing_repeats() {
        let mut repeater = Repeater::new();
        repeater.event(KeyEvent::Pressed(A), ms(0));
        assert_eq!(repeater.next_at(), None);
        // And turning it off forgets a held key.
        let mut repeater = repeating();
        repeater.event(KeyEvent::Pressed(A), ms(0));
        repeater.set(None);
        assert_eq!(repeater.due(ms(1000)), None);
    }
}
