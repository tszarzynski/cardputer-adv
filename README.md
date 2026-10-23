# cardputer-adv

Board support for the [M5Stack Cardputer ADV](https://docs.m5stack.com/en/core/Cardputer-Adv) (ESP32-S3) on [esp-hal](https://github.com/esp-rs/esp-hal) 1.x.

- `display(DisplayPins)` sets up the 1.14" ST7789V2 LCD and turns the backlight on. You get it back as a 240×135 RGB565 [embedded-graphics](https://docs.rs/embedded-graphics) `DrawTarget`, in landscape with the keyboard below.
- `keyboard(KeyboardPins)` sets up the 56-key TCA8418 keyboard on I2C, with Shift and Fn layers from [cardputer-adv-keyboard](https://crates.io/crates/cardputer-adv-keyboard). Fn + `;` `.` `,` `/` gives the arrow keys.

```rust
let (mut display, _backlight) = cardputer_adv::display(DisplayPins {
    spi: p.SPI2, sck: p.GPIO36, mosi: p.GPIO35, cs: p.GPIO37,
    dc: p.GPIO34, reset: p.GPIO33, backlight: p.GPIO38,
})?;
let mut keyboard = cardputer_adv::keyboard(KeyboardPins { i2c: p.I2C0, sda: p.GPIO8, scl: p.GPIO9 })?;
display.clear(Rgb565::BLACK)?;
for key in keyboard.inputs()? { /* … */ }
```

## Pins

| Function | GPIO |
|---|---|
| LCD SCK / MOSI / CS / DC / RST | 36 / 35 / 37 / 34 / 33 |
| LCD backlight (also powers the RGB LED) | 38 |
| I2C SDA / SCL (keyboard, ES8311 codec, BMI270 IMU) | 8 / 9 |
| Keyboard interrupt | 11 |

The keyboard currently takes the I2C bus for itself. The codec and IMU on the same bus aren't supported yet.

## Status

The display settings (panel offset 52/40, 90° rotation, colour inversion) come from working ADV firmware ([iwan-inc/cardputer-rust](https://github.com/iwan-inc/cardputer-rust)). This crate itself hasn't been run on hardware yet.

## Licence

MIT OR Apache-2.0.
