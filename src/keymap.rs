//! The keyboard's layout: which key a TCA8418 event is, and what it types
//! with Shift and Fn held. The layout and its layers are those of
//! [cardputer-adv-keyboard](https://crates.io/crates/cardputer-adv-keyboard)
//! (MIT OR Apache-2.0).

use crate::keys::{Arrow, KeyEvent, KeyInput, Modifier};

/// What a key is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    /// A character, and the one Shift gives.
    Char(char, char),
    Modifier(Modifier),
    Enter,
    Backspace,
    Tab,
}

/// The keys as they're laid out, four rows of 14.
#[rustfmt::skip]
const LAYOUT: [[Key; 14]; 4] = {
    use Key::{Backspace, Char as C, Enter, Modifier as M, Tab};
    use Modifier::{Alt, Ctrl, Fn, Opt, Shift};
    [
        [C('`', '~'), C('1', '!'), C('2', '@'), C('3', '#'), C('4', '$'), C('5', '%'), C('6', '^'),
         C('7', '&'), C('8', '*'), C('9', '('), C('0', ')'), C('-', '_'), C('=', '+'), Backspace],
        [Tab, C('q', 'Q'), C('w', 'W'), C('e', 'E'), C('r', 'R'), C('t', 'T'), C('y', 'Y'),
         C('u', 'U'), C('i', 'I'), C('o', 'O'), C('p', 'P'), C('[', '{'), C(']', '}'), C('\\', '|')],
        [M(Fn), M(Shift), C('a', 'A'), C('s', 'S'), C('d', 'D'), C('f', 'F'), C('g', 'G'),
         C('h', 'H'), C('j', 'J'), C('k', 'K'), C('l', 'L'), C(';', ':'), C('\'', '"'), Enter],
        [M(Ctrl), M(Opt), M(Alt), C('z', 'Z'), C('x', 'X'), C('c', 'C'), C('v', 'V'),
         C('b', 'B'), C('n', 'N'), C('m', 'M'), C(',', '<'), C('.', '>'), C('/', '?'), C(' ', ' ')],
    ]
};

/// The key a TCA8418 key number (1–80: its matrix row × 10 + column + 1)
/// is. The matrix is wired with each of its rows across two of the
/// layout's columns, and its columns 0–3 and 4–7 down the layout's rows;
/// it has no columns 8 and 9.
fn key(number: u8) -> Option<Key> {
    let index = number.checked_sub(1).filter(|&index| index < 80)?;
    let (row, column) = (usize::from(index / 10), usize::from(index % 10));
    if column >= 8 {
        return None;
    }
    LAYOUT
        .get(column % 4)?
        .get(row * 2 + usize::from(column >= 4))
        .copied()
}

/// What a key types on the Fn layer, if anything.
const fn fn_layer(key: Key) -> Option<KeyInput> {
    Some(match key {
        Key::Char('`', _) => KeyInput::Escape,
        Key::Char('\\', _) | Key::Backspace => KeyInput::Delete,
        Key::Char(';', _) => KeyInput::Arrow(Arrow::Up),
        Key::Char(',', _) => KeyInput::Arrow(Arrow::Left),
        Key::Char('.', _) => KeyInput::Arrow(Arrow::Down),
        Key::Char('/', _) => KeyInput::Arrow(Arrow::Right),
        _ => return None,
    })
}

/// What `key` types, with Shift or Fn held or not.
const fn typed(key: Key, shift: bool, fn_held: bool) -> KeyInput {
    if fn_held && let Some(input) = fn_layer(key) {
        return input;
    }
    match key {
        Key::Char(plain, shifted) => KeyInput::Char(if shift { shifted } else { plain }),
        Key::Modifier(modifier) => KeyInput::Modifier(modifier),
        Key::Enter => KeyInput::Enter,
        Key::Backspace => KeyInput::Backspace,
        Key::Tab => KeyInput::Tab,
    }
}

/// Turns the TCA8418's key events into what they type, following which
/// keys are held (Shift and Fn among them).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Keymap {
    /// The keys held: a bit for each key number.
    held: u128,
    /// Of those, the keys pressed with Shift held, and with Fn: what they
    /// typed, for their releases to say the same.
    shifted: u128,
    fn_layer: u128,
}

impl Keymap {
    /// No keys held.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            held: 0,
            shifted: 0,
            fn_layer: 0,
        }
    }

    /// The key event a `KEY_EVENT_A` byte (bit 7 set for a press, then the
    /// key number) is, with what the key types: as Shift and Fn are now for
    /// a press, and as they were at its press for a release. `None` for a
    /// number that isn't a key. A modifier's own press types
    /// [`KeyInput::Modifier`].
    pub fn event(&mut self, event: u8) -> Option<KeyEvent> {
        let number = event & 0x7F;
        let key = key(number)?;
        let bit = 1 << number;
        if event & 0x80 != 0 {
            self.held |= bit;
            let shift = self.holds(Key::Modifier(Modifier::Shift));
            let fn_held = self.holds(Key::Modifier(Modifier::Fn));
            self.shifted = if shift {
                self.shifted | bit
            } else {
                self.shifted & !bit
            };
            self.fn_layer = if fn_held {
                self.fn_layer | bit
            } else {
                self.fn_layer & !bit
            };
            Some(KeyEvent::Pressed(typed(key, shift, fn_held)))
        } else {
            self.held &= !bit;
            let typed = typed(key, self.shifted & bit != 0, self.fn_layer & bit != 0);
            Some(KeyEvent::Released(typed))
        }
    }

    /// Whether a key that types `input`, with or without Shift or Fn, is
    /// held.
    #[must_use]
    pub fn is_held(&self, input: KeyInput) -> bool {
        self.held_keys().any(|key| {
            [(false, false), (true, false), (false, true)]
                .into_iter()
                .any(|(shift, fn_held)| typed(key, shift, fn_held) == input)
        })
    }

    fn holds(&self, key: Key) -> bool {
        self.held_keys().any(|held| held == key)
    }

    fn held_keys(&self) -> impl Iterator<Item = Key> + use<'_> {
        (1..=80)
            .filter(|&number| self.held & 1 << number != 0)
            .filter_map(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A press of key `number`.
    const fn press(number: u8) -> u8 {
        number | 0x80
    }

    /// What `event` types, if it's a press.
    fn typed(keymap: &mut Keymap, event: u8) -> Option<KeyInput> {
        keymap
            .event(event)
            .filter(|event| event.is_press())
            .map(KeyEvent::input)
    }

    /// The key numbers of the keys used here: matrix row × 10 + column + 1.
    const FN: u8 = 3;
    const SHIFT: u8 = 7;
    const A: u8 = 13;
    const SEMICOLON: u8 = 57;

    #[test]
    fn a_key_types_its_character_and_shift_the_upper_one() {
        let mut keymap = Keymap::default();
        assert_eq!(typed(&mut keymap, press(A)), Some(KeyInput::Char('a')));
        assert_eq!(typed(&mut keymap, A), None);
        assert_eq!(
            typed(&mut keymap, press(SHIFT)),
            Some(KeyInput::Modifier(Modifier::Shift))
        );
        assert_eq!(typed(&mut keymap, press(A)), Some(KeyInput::Char('A')));
        assert_eq!(typed(&mut keymap, SHIFT), None);
        assert_eq!(typed(&mut keymap, press(A)), Some(KeyInput::Char('a')));
    }

    #[test]
    fn fn_gives_the_arrows_over_shift() {
        let mut keymap = Keymap::default();
        typed(&mut keymap, press(FN));
        typed(&mut keymap, press(SHIFT));
        assert_eq!(
            typed(&mut keymap, press(SEMICOLON)),
            Some(KeyInput::Arrow(Arrow::Up))
        );
        // Nothing on the Fn layer: Shift still applies.
        assert_eq!(typed(&mut keymap, press(A)), Some(KeyInput::Char('A')));
    }

    #[test]
    fn a_key_is_held_from_its_press_to_its_release() {
        let mut keymap = Keymap::default();
        assert!(!keymap.is_held(KeyInput::Char('a')));
        typed(&mut keymap, press(A));
        assert!(keymap.is_held(KeyInput::Char('a')));
        assert!(keymap.is_held(KeyInput::Char('A')), "the same key");
        assert!(!keymap.is_held(KeyInput::Char('s')));
        typed(&mut keymap, A);
        assert!(!keymap.is_held(KeyInput::Char('a')));
    }

    #[test]
    fn modifiers_and_fn_layer_keys_are_held_too() {
        let mut keymap = Keymap::default();
        typed(&mut keymap, press(FN));
        typed(&mut keymap, press(SEMICOLON));
        assert!(keymap.is_held(KeyInput::Modifier(Modifier::Fn)));
        assert!(keymap.is_held(KeyInput::Arrow(Arrow::Up)));
        assert!(keymap.is_held(KeyInput::Char(';')), "the same key");
        assert!(!keymap.is_held(KeyInput::Modifier(Modifier::Shift)));
        typed(&mut keymap, FN);
        // The key is still down, whatever it would type now.
        assert!(keymap.is_held(KeyInput::Arrow(Arrow::Up)));
    }

    #[test]
    fn a_release_types_what_its_press_did() {
        let mut keymap = Keymap::default();
        assert_eq!(
            keymap.event(press(A)),
            Some(KeyEvent::Pressed(KeyInput::Char('a')))
        );
        keymap.event(press(SHIFT));
        assert_eq!(
            keymap.event(A),
            Some(KeyEvent::Released(KeyInput::Char('a'))),
            "pressed before Shift"
        );
        keymap.event(press(A));
        keymap.event(SHIFT);
        assert_eq!(
            keymap.event(A),
            Some(KeyEvent::Released(KeyInput::Char('A'))),
            "pressed with Shift"
        );
        keymap.event(press(FN));
        keymap.event(press(SEMICOLON));
        keymap.event(FN);
        assert_eq!(
            keymap.event(SEMICOLON),
            Some(KeyEvent::Released(KeyInput::Arrow(Arrow::Up)))
        );
        assert_eq!(
            keymap.event(SHIFT),
            Some(KeyEvent::Released(KeyInput::Modifier(Modifier::Shift)))
        );
    }

    #[test]
    fn numbers_off_the_keyboard_type_nothing() {
        let mut keymap = Keymap::default();
        assert_eq!(typed(&mut keymap, 0), None);
        assert_eq!(typed(&mut keymap, press(0)), None);
        assert_eq!(typed(&mut keymap, press(81)), None);
        assert_eq!(typed(&mut keymap, press(97)), None);
        // Row 7 of the matrix is past the layout's 14 columns, and it has
        // no column 8.
        assert_eq!(typed(&mut keymap, press(71)), None);
        assert_eq!(typed(&mut keymap, press(9)), None);
    }

    #[test]
    fn every_layout_position_has_one_key_number() {
        let found = (1..=80).filter(|&number| key(number).is_some()).count();
        assert_eq!(found, 56);
    }
}
