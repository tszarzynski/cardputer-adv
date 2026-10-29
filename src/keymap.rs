//! The keyboard's layout: which key a TCA8418 event is, and what it types
//! with Shift and Fn held. The layout and its layers are those of
//! [cardputer-adv-keyboard](https://crates.io/crates/cardputer-adv-keyboard)
//! (MIT OR Apache-2.0).

use crate::keys::{Arrow, KeyInput, Modifier};

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

/// Turns the TCA8418's key events into what they type, following which of
/// Shift and Fn are held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Keymap {
    shift: bool,
    fn_held: bool,
}

impl Keymap {
    /// What a key event (a `KEY_EVENT_A` byte: bit 7 set for a press, then
    /// the key number) types: `None` for a release, or a number that isn't
    /// a key. A modifier's own press types [`KeyInput::Modifier`].
    pub fn input(&mut self, event: u8) -> Option<KeyInput> {
        let pressed = event & 0x80 != 0;
        let key = key(event & 0x7F)?;
        match key {
            Key::Modifier(Modifier::Shift) => self.shift = pressed,
            Key::Modifier(Modifier::Fn) => self.fn_held = pressed,
            _ => {}
        }
        if !pressed {
            return None;
        }
        if self.fn_held
            && let Some(input) = fn_layer(key)
        {
            return Some(input);
        }
        Some(match key {
            Key::Char(plain, shifted) => KeyInput::Char(if self.shift { shifted } else { plain }),
            Key::Modifier(modifier) => KeyInput::Modifier(modifier),
            Key::Enter => KeyInput::Enter,
            Key::Backspace => KeyInput::Backspace,
            Key::Tab => KeyInput::Tab,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A press of key `number`.
    const fn press(number: u8) -> u8 {
        number | 0x80
    }

    /// The key numbers of the keys used here: matrix row × 10 + column + 1.
    const FN: u8 = 3;
    const SHIFT: u8 = 7;
    const A: u8 = 13;
    const SEMICOLON: u8 = 57;

    #[test]
    fn a_key_types_its_character_and_shift_the_upper_one() {
        let mut keymap = Keymap::default();
        assert_eq!(keymap.input(press(A)), Some(KeyInput::Char('a')));
        assert_eq!(keymap.input(A), None);
        assert_eq!(
            keymap.input(press(SHIFT)),
            Some(KeyInput::Modifier(Modifier::Shift))
        );
        assert_eq!(keymap.input(press(A)), Some(KeyInput::Char('A')));
        assert_eq!(keymap.input(SHIFT), None);
        assert_eq!(keymap.input(press(A)), Some(KeyInput::Char('a')));
    }

    #[test]
    fn fn_gives_the_arrows_over_shift() {
        let mut keymap = Keymap::default();
        keymap.input(press(FN));
        keymap.input(press(SHIFT));
        assert_eq!(
            keymap.input(press(SEMICOLON)),
            Some(KeyInput::Arrow(Arrow::Up))
        );
        // Nothing on the Fn layer: Shift still applies.
        assert_eq!(keymap.input(press(A)), Some(KeyInput::Char('A')));
    }

    #[test]
    fn numbers_off_the_keyboard_type_nothing() {
        let mut keymap = Keymap::default();
        assert_eq!(keymap.input(0), None);
        assert_eq!(keymap.input(press(0)), None);
        assert_eq!(keymap.input(press(81)), None);
        assert_eq!(keymap.input(press(97)), None);
        // Row 7 of the matrix is past the layout's 14 columns, and it has
        // no column 8.
        assert_eq!(keymap.input(press(71)), None);
        assert_eq!(keymap.input(press(9)), None);
    }

    #[test]
    fn every_layout_position_has_one_key_number() {
        let found = (1..=80).filter(|&number| key(number).is_some()).count();
        assert_eq!(found, 56);
    }
}
