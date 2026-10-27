//! The RGB LED: a WS2812 driven by an RMT channel.

use embassy_time::{Duration, Timer};
use embedded_graphics_core::pixelcolor::{Rgb888, RgbColor};
use esp_hal::{
    Async,
    gpio::Level,
    peripherals::GPIO21,
    rmt::{PulseCode, TxChannelConfig, TxChannelCreator},
};

use crate::{Error, remote::RmtTx};

/// A WS2812 bit in RMT ticks: a 0 is high for 0.4 µs then low for 0.8 µs,
/// a 1 high for 0.8 µs then low for 0.4 µs (each within its 0.15 µs
/// tolerance).
const LED_ZERO: PulseCode = PulseCode::new(Level::High, 4, Level::Low, 8);
const LED_ONE: PulseCode = PulseCode::new(Level::High, 8, Level::Low, 4);

/// How long the line stays low to latch a colour (WS2812B: over 280 µs).
const LED_LATCH: Duration = Duration::from_micros(300);

/// The RGB LED: a WS2812 on GPIO21, driven by an RMT channel.
///
/// It's powered through the backlight's switch (GPIO38): it's lit only
/// while the [`Backlight`] is, and dims with it. After the backlight has
/// been off, set the colour again.
///
/// [`Backlight`]: crate::Backlight
#[derive(Debug)]
pub struct RgbLed {
    channel: RmtTx,
}

impl RgbLed {
    /// Sets the colour; black turns the LED off. Returns once the LED has
    /// taken it (about 0.3 ms).
    ///
    /// # Errors
    ///
    /// If the RMT fails to send it.
    pub async fn set_color(&mut self, color: Rgb888) -> Result<(), Error> {
        // Green, red, blue, each most significant bit first; then the end
        // marker.
        let grb = (u32::from(color.g()) << 16) | (u32::from(color.r()) << 8) | u32::from(color.b());
        let mut codes = [PulseCode::end_marker(); 25];
        for (bit, code) in (0..24).rev().zip(&mut codes) {
            *code = if (grb >> bit) & 1 == 1 {
                LED_ONE
            } else {
                LED_ZERO
            };
        }
        self.channel
            .transmit(&codes)
            .await
            .map_err(|_| Error::Led)?;
        Timer::after(LED_LATCH).await;
        Ok(())
    }
}

/// Sets up the RGB LED on an RMT transmit channel of [`rmt`]'s. It starts
/// as it was, usually off.
///
/// # Errors
///
/// If the channel can't be configured.
///
/// [`rmt`]: crate::rmt
pub fn rgb_led(
    channel: impl TxChannelCreator<'static, Async>,
    pin: GPIO21<'static>,
) -> Result<RgbLed, Error> {
    let config = TxChannelConfig::default()
        .with_clk_divider(1)
        .with_idle_output(true)
        .with_idle_output_level(Level::Low);
    let channel = channel
        .configure_tx(&config)
        .map_err(|_| Error::Rmt)?
        .with_pin(pin);
    Ok(RgbLed { channel })
}
