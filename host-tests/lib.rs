//! The crate's pure logic, from its own source files, built for the host
//! so the tests in them run: the battery estimate and the other unit
//! conversions, the RGB LED's and the IR codes' pulses, the framebuffer's
//! change tracking, the keyboard's layout and its controller's
//! reading (against a fake TCA8418), the motion sensor's configuration
//! upload, the speaker's tones, and screenshots' encoding.
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
#[path = "../src/repeat.rs"]
mod repeat;
#[path = "../src/screenshot.rs"]
mod screenshot;
#[path = "../src/tca8418.rs"]
mod tca8418;
#[path = "../src/tone.rs"]
mod tone;
#[path = "../src/units.rs"]
mod units;

pub use bmi270::{AccelerometerRange, GyroscopeRange, ImuMode, ImuRate};
pub use frame::Frame;
pub use keymap::Keymap;
pub use panel::{HEIGHT, WIDTH};
pub use units::{Decibels, Millivolts, Percent};
