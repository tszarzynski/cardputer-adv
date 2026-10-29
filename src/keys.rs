//! What a key press types, and keys going down and coming up:
//! [`Keyboard::wait_for_inputs`] and [`Keyboard::wait_for_events`] return
//! these.
//!
//! [`Keyboard::wait_for_inputs`]: crate::Keyboard::wait_for_inputs
//! [`Keyboard::wait_for_events`]: crate::Keyboard::wait_for_events

use core::fmt;

/// What a key press types, with Shift and Fn applied.
///
/// Fn takes precedence: Fn + Shift + `;` is [`Arrow::Up`]. A key with
/// nothing on its Fn layer types as it would without Fn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum KeyInput {
    /// A printable character (Shift gives the upper one on the key).
    Char(char),
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Delete: Fn + Backspace.
    Delete,
    /// Tab.
    Tab,
    /// Escape: Fn + `` ` ``.
    Escape,
    /// An arrow: Fn + `;` (up), `,` (left), `.` (down) or `/` (right).
    Arrow(Arrow),
    /// A modifier on its own: pressing Shift, say, before the key it shifts.
    Modifier(Modifier),
}

/// The arrow keys' directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[expect(missing_docs, reason = "the directions are their names")]
#[non_exhaustive]
pub enum Arrow {
    Up,
    Down,
    Left,
    Right,
}

/// The modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[expect(missing_docs, reason = "the keys are their labels")]
#[non_exhaustive]
pub enum Modifier {
    Shift,
    Fn,
    Ctrl,
    Opt,
    Alt,
}

impl KeyInput {
    /// The character it puts in text: a [`Char`](Self::Char)'s, `'\n'` for
    /// Enter and `'\t'` for Tab. `None` for the keys that edit or move
    /// instead, and the modifiers.
    #[must_use]
    pub const fn char(self) -> Option<char> {
        match self {
            Self::Char(c) => Some(c),
            Self::Enter => Some('\n'),
            Self::Tab => Some('\t'),
            _ => None,
        }
    }
}

/// The key's name, as a label or a hint would show it: a character as
/// itself (Space as `Space`), and the other keys by name (`Enter`, `Esc`,
/// `Up`, `Shift`, …).
impl fmt::Display for KeyInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Char(' ') => f.write_str("Space"),
            Self::Char(c) => write!(f, "{c}"),
            Self::Enter => f.write_str("Enter"),
            Self::Backspace => f.write_str("Backspace"),
            Self::Delete => f.write_str("Delete"),
            Self::Tab => f.write_str("Tab"),
            Self::Escape => f.write_str("Esc"),
            Self::Arrow(arrow) => write!(f, "{arrow}"),
            Self::Modifier(modifier) => write!(f, "{modifier}"),
        }
    }
}

impl fmt::Display for Arrow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
        })
    }
}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Shift => "Shift",
            Self::Fn => "Fn",
            Self::Ctrl => "Ctrl",
            Self::Opt => "Opt",
            Self::Alt => "Alt",
        })
    }
}

/// A key going down or coming up, and what it types.
///
/// A release carries what the key typed when it was pressed, so it always
/// matches its press: press `a`, then Shift, then let go of `a`, and the
/// release is still `Char('a')`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum KeyEvent {
    /// A key pressed.
    Pressed(KeyInput),
    /// A key released.
    Released(KeyInput),
}

impl KeyEvent {
    /// What the key types.
    #[must_use]
    pub const fn input(self) -> KeyInput {
        match self {
            Self::Pressed(input) | Self::Released(input) => input,
        }
    }

    /// Whether it's a press.
    #[must_use]
    pub const fn is_press(self) -> bool {
        matches!(self, Self::Pressed(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keys_have_a_character_and_the_others_dont() {
        assert_eq!(KeyInput::Char('a').char(), Some('a'));
        assert_eq!(KeyInput::Enter.char(), Some('\n'));
        assert_eq!(KeyInput::Tab.char(), Some('\t'));
        assert_eq!(KeyInput::Backspace.char(), None);
        assert_eq!(KeyInput::Arrow(Arrow::Up).char(), None);
        assert_eq!(KeyInput::Modifier(Modifier::Shift).char(), None);
    }

    #[test]
    fn keys_display_as_their_names() {
        let name = |input: KeyInput| input.to_string();
        assert_eq!(name(KeyInput::Char('A')), "A");
        assert_eq!(name(KeyInput::Char(' ')), "Space");
        assert_eq!(name(KeyInput::Escape), "Esc");
        assert_eq!(name(KeyInput::Arrow(Arrow::Left)), "Left");
        assert_eq!(name(KeyInput::Modifier(Modifier::Ctrl)), "Ctrl");
    }

    #[test]
    fn an_event_tells_its_key_and_whether_it_went_down() {
        let press = KeyEvent::Pressed(KeyInput::Tab);
        let release = KeyEvent::Released(KeyInput::Tab);
        assert_eq!(press.input(), KeyInput::Tab);
        assert_eq!(release.input(), KeyInput::Tab);
        assert!(press.is_press());
        assert!(!release.is_press());
    }
}
