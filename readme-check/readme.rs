//! The README's examples and the crate docs' quick start, compiled so they
//! can't go stale (`just readme`, part of `just check`). build.rs takes them
//! out of their files; each function here gives one what the text around it
//! assumes: the peripherals, and what the examples before it set up.
//! Nothing here runs: `cargo check` only compiles it.

#![no_std]
#![no_main]
#![expect(dead_code, reason = "the examples are compiled, not run")]
#![expect(
    unused_variables,
    unused_mut,
    reason = "the examples stop where the reader's own code would go on"
)]

use core::panic::PanicInfo;

use cardputer_adv::{
    Backlight, Codec, Display, Error, Frame, FrameDisplay, I2cBus, Keyboard, Percent, display_pins,
};
use embedded_graphics::{pixelcolor::Rgb565, prelude::*};
use esp_hal::peripherals::Peripherals;

include!(concat!(env!("OUT_DIR"), "/count.rs"));
const _: () = assert!(
    README == 6 && CRATE_DOCS == 1,
    "an example without a function here, or a function without its example"
);

/// Example `$n` from `$source` (`readme` or `crate_docs`), as a block.
macro_rules! example {
    ($source:literal, $n:literal) => {
        include!(concat!(env!("OUT_DIR"), "/", $source, "_", $n, ".rs"))
    };
}

/// The crate docs' quick start, which starts esp-hal itself.
async fn quick_start_in_the_docs() -> Result<(), Error> {
    example!("crate_docs", 1);
    Ok(())
}

/// The README's first: the display and the keyboard.
async fn display_and_keyboard(p: Peripherals) -> Result<(), Error> {
    example!("readme", 1)
}

/// The shared bus, the motion sensor, the speaker and the codec.
async fn bus_motion_and_speaker(p: Peripherals) -> Result<(), Error> {
    example!("readme", 2);
    Ok(())
}

/// The microphone, the RGB LED and the IR transmitter, on the bus from
/// the one before.
async fn microphone_led_and_ir(p: Peripherals, bus: &'static I2cBus) -> Result<(), Error> {
    example!("readme", 3);
    Ok(())
}

/// The framebuffer.
async fn framebuffer(p: Peripherals) -> Result<(), Error> {
    example!("readme", 4)
}

/// Sleeping, with the parts the examples before it set up.
async fn sleeping(
    p: Peripherals,
    mut backlight: Backlight,
    mut display: FrameDisplay,
    mut codec: Codec,
    mut keyboard: Keyboard,
) -> Result<(), Error> {
    example!("readme", 5);
    Ok(())
}

/// Screenshots, of a framebuffer and of drawing straight to the screen.
async fn screenshots(
    mut display: FrameDisplay,
    frame: &'static mut Frame,
    mut lcd: Display,
) -> Result<(), Error> {
    example!("readme", 6);
    Ok(())
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}
