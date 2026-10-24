# cardputer-adv

Board support for the [M5Stack Cardputer ADV](https://docs.m5stack.com/en/core/Cardputer-Adv) (ESP32-S3) on [esp-hal](https://github.com/esp-rs/esp-hal) 1.x.

- `display(DisplayPins)` sets up the 1.14" ST7789V2 LCD and turns the backlight on. You get it back as a 240×135 RGB565 [embedded-graphics](https://docs.rs/embedded-graphics) `DrawTarget`, in landscape with the keyboard below, and a `Backlight`. `Backlight::set_percent` dims it with 24 kHz PWM on LEDC; the RGB LED, on the same pin, dims too.
- `keyboard(KeyboardPins)` sets up the 56-key TCA8418 keyboard on I2C, with Shift and Fn layers from [cardputer-adv-keyboard](https://crates.io/crates/cardputer-adv-keyboard). Fn + `;` `.` `,` `/` gives the arrow keys.
- `Keyboard::wait_inputs().await` sleeps until the TCA8418 pulls its interrupt line (GPIO11) low, then returns the keys pressed.
  - It waits for the low level, not an edge, so events that arrive while you're busy aren't lost.
  - A 1 s fallback poll covers a missed interrupt; `missed_interrupts()` counts how often that poll was needed.
- `battery(BatteryPins)` measures the battery voltage (`Battery::millivolts()`) on GPIO10 through the board's 2:1 divider, as M5Stack's M5Unified does. The board can't tell whether it's charging: on USB with the power switch on, it reads the charging voltage.

```rust
let (mut display, mut backlight) = cardputer_adv::display(DisplayPins {
    spi: p.SPI2, sck: p.GPIO36, mosi: p.GPIO35, cs: p.GPIO37,
    dc: p.GPIO34, reset: p.GPIO33, backlight: p.GPIO38, ledc: p.LEDC,
})?;
let mut keyboard = cardputer_adv::keyboard(KeyboardPins {
    i2c: p.I2C0, sda: p.GPIO8, scl: p.GPIO9, interrupt: p.GPIO11,
})?;
display.clear(Rgb565::BLACK)?;
loop {
    for key in keyboard.wait_inputs().await? { /* … */ }
}
```

## Pins

| Function | GPIO |
|---|---|
| LCD SCK / MOSI / CS / DC / RST | 36 / 35 / 37 / 34 / 33 |
| LCD backlight, PWM (also powers the RGB LED) | 38 |
| Battery voltage (ADC1, 2:1 divider) | 10 |
| I2C SDA / SCL (keyboard, ES8311 codec, BMI270 IMU) | 8 / 9 |
| Keyboard interrupt | 11 |

The keyboard currently takes the I2C bus for itself. The codec and IMU on the same bus aren't supported yet.

## Status

The display settings (panel offset 52/40, 90° rotation, colour inversion) come from working ADV firmware ([iwan-inc/cardputer-rust](https://github.com/iwan-inc/cardputer-rust)). The display and the keyboard, including its interrupt line, work on a Cardputer ADV (tested October 2026). The battery reading is new and not yet checked against a multimeter.

## Licence

MIT OR Apache-2.0.
