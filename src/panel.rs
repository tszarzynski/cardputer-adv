//! The LCD panel's geometry.

/// Display width in landscape.
pub const WIDTH: u16 = 240;
/// Display height in landscape.
pub const HEIGHT: u16 = 135;

/// Where the panel's 135×240 pixels sit in the ST7789's 240×320 memory.
pub const PANEL_OFFSET: (u16, u16) = (52, 40);
