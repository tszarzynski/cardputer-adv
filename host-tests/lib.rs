//! The crate's pure logic, from its own source files, built for the host
//! so the tests in them run: the battery estimate and the other unit
//! conversions, the RGB LED's and the IR codes' pulses, the framebuffer's
//! change tracking, the keyboard's layout and the motion sensor's
//! configuration upload.
//!
//! The modules sit at the root, as in the crate, so their `crate::` paths
//! resolve the same here.

#![expect(
    dead_code,
    reason = "what only the hardware modules use, such as the frame's flush planning"
)]

#[path = "../src/bmi270.rs"]
mod bmi270;
#[path = "../src/convert.rs"]
mod convert;
#[path = "../src/frame.rs"]
mod frame;
#[path = "../src/keymap.rs"]
mod keymap;
#[path = "../src/keys.rs"]
pub mod keys;
#[path = "../src/panel.rs"]
mod panel;
#[path = "../src/pulses.rs"]
mod pulses;
#[path = "../src/units.rs"]
mod units;

pub use frame::Frame;
pub use keymap::Keymap;
pub use panel::{HEIGHT, WIDTH};
pub use units::{Decibels, Millivolts, Percent};
