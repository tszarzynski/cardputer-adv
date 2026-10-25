# cardputer-adv

Board support for the [M5Stack Cardputer ADV](https://docs.m5stack.com/en/core/Cardputer-Adv) (ESP32-S3) on [esp-hal](https://github.com/esp-rs/esp-hal) 1.x.

- `display(DisplayPins)` sets up the 1.14" ST7789V2 LCD and turns the backlight on. You get it back as a 240×135 RGB565 [embedded-graphics](https://docs.rs/embedded-graphics) `DrawTarget`, in landscape with the keyboard below, and a `Backlight`. `Backlight::set_percent` dims it with 256 Hz PWM on LEDC (as M5GFX does; the LED driver doesn't follow fast PWM); the RGB LED, on the same pin, dims too.
- `keyboard(KeyboardPins)` sets up the 56-key TCA8418 keyboard on I2C, with Shift and Fn layers from [cardputer-adv-keyboard](https://crates.io/crates/cardputer-adv-keyboard). Fn + `;` `.` `,` `/` gives the arrow keys.
- `Keyboard::wait_inputs().await` sleeps until the TCA8418 pulls its interrupt line (GPIO11) low, then returns the keys pressed.
  - It waits for the low level, not an edge, so events that arrive while you're busy aren't lost.
  - A 1 s fallback poll covers a missed interrupt; `missed_interrupts()` counts how often that poll was needed.
- `imu(&I2cBus)` finds the BMI270 motion sensor (at 0x68 or 0x69), uploads its configuration through [bmi2](https://crates.io/crates/bmi2) and turns on its accelerometer at ±8 g and 100 Hz. `Imu::acceleration()` reads it in milli-g.
- `speaker(SpeakerPins)` starts a 16-bit, 44.1 kHz I2S stream over DMA to the ES8311 codec, playing silence. `enable_codec(&I2cBus)` then turns on the codec's DAC path to the NS4150B amplifier. `Speaker::run(fill).await` keeps the stream fed with the mono samples your closure writes.
  - The buffer holds 23 ms. If anything else holds the CPU for longer (drawing a whole frame can take 30–40 ms), run the speaker on a higher-priority executor, such as esp-rtos's `InterruptExecutor`.
- `i2c(I2cPins)` sets up the I2C bus that the keyboard, codec and motion sensor share. Pass it to `keyboard_on`, `imu` and `enable_codec`. `keyboard(KeyboardPins)` sets the bus up itself, for when the keyboard is all you need.
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

With the motion sensor and the speaker, set the shared bus up first:

```rust
let bus = cardputer_adv::i2c(I2cPins { i2c: p.I2C0, sda: p.GPIO8, scl: p.GPIO9 })?;
let mut keyboard = cardputer_adv::keyboard_on(bus, p.GPIO11)?;
let mut imu = cardputer_adv::imu(bus)?;
let [x, y, z] = imu.acceleration()?; // milli-g

// In a task (ideally on a higher-priority executor):
let speaker = cardputer_adv::speaker(SpeakerPins {
    i2s: p.I2S0, dma: p.DMA_CH0, bclk: p.GPIO41, ws: p.GPIO43, dout: p.GPIO42,
})?;
// …and once it's running (the codec clocks itself from the stream):
cardputer_adv::enable_codec(bus)?;
let error = speaker.run(|samples| samples.fill(0)).await;
```

## Pins

| Function | GPIO |
|---|---|
| LCD SCK / MOSI / CS / DC / RST | 36 / 35 / 37 / 34 / 33 |
| LCD backlight, PWM (also powers the RGB LED) | 38 |
| Battery voltage (ADC1, 2:1 divider) | 10 |
| I2C SDA / SCL (keyboard, ES8311 codec, BMI270 IMU) | 8 / 9 |
| Keyboard interrupt | 11 |
| I2S BCLK / WS / DOUT, to the ES8311 (no MCLK: it clocks from BCLK) | 41 / 43 / 42 |

The I2C bus is a `RefCell` shared by `RefCellDevice`s. Each device borrows it only during a transfer, never across an `.await`, so it works from any tasks on one executor. It isn't `Sync`, so it can't be used from another executor or core.

## Status

The display settings (panel offset 52/40, 90° rotation, colour inversion) come from working ADV firmware ([iwan-inc/cardputer-rust](https://github.com/iwan-inc/cardputer-rust)). The display and the keyboard, including its interrupt line, work on a Cardputer ADV (tested October 2026). So do the motion sensor (found at 0x68) and the speaker. The ES8311 sequence is M5Unified's Cardputer ADV speaker enable, as [espp](https://github.com/esp-cpp/espp/tree/main/components/m5stack-cardputer)'s driver uses it. The battery reading is new and not yet checked against a multimeter.

## Licence

MIT OR Apache-2.0.
