//! What a key press types: [`Keyboard::wait_for_inputs`] returns these.
//!
//! [`Keyboard::wait_for_inputs`]: crate::Keyboard::wait_for_inputs

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
