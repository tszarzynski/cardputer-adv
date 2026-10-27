//! The RMT (remote control) peripheral, which the RGB LED and the IR
//! transmitter share.

use esp_hal::{
    Async,
    peripherals::RMT,
    rmt::{Channel, Rmt, Tx},
    time::Rate,
};

use crate::Error;

/// The RMT's clock, as [`rmt`] sets it up: the RGB LED's and the IR
/// transmitter's timings are counted in its ticks (0.1 µs).
pub const RMT_FREQUENCY: Rate = Rate::from_mhz(10);

/// Sets up the RMT (remote control) peripheral at [`RMT_FREQUENCY`].
///
/// Pass one of its transmit channels (`channel0` to `channel3`) each to
/// [`rgb_led`] and [`ir_transmitter`]; the others stay free.
///
/// # Errors
///
/// If the RMT can't be clocked at [`RMT_FREQUENCY`] (it can, from the
/// ESP32-S3's 80 MHz).
///
/// [`rgb_led`]: crate::rgb_led
/// [`ir_transmitter`]: crate::ir_transmitter
pub fn rmt(rmt: RMT<'static>) -> Result<Rmt<'static, Async>, Error> {
    Rmt::new(rmt, RMT_FREQUENCY)
        .map(Rmt::into_async)
        .map_err(|_| Error::Rmt)
}

/// A transmit channel of the RMT, as the RGB LED and the IR transmitter
/// hold it.
pub type RmtTx = Channel<'static, Async, Tx>;
