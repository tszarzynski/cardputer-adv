//! The crate's pure logic, from its own source files, built for the host
//! so the tests in them run: the battery estimate and the other unit
//! conversions, the RGB LED's and the IR codes' pulses, and the
//! framebuffer's change tracking.
//!
//! The modules sit at the root, as in the crate, so their `crate::` paths
//! resolve the same here.

#![expect(
    dead_code,
    reason = "what only the hardware modules use, such as the frame's flush planning"
)]

#[path = "../src/convert.rs"]
mod convert;
#[path = "../src/frame.rs"]
mod frame;
#[path = "../src/panel.rs"]
mod panel;
#[path = "../src/pulses.rs"]
mod pulses;

pub use frame::Frame;
pub use panel::{HEIGHT, WIDTH};
