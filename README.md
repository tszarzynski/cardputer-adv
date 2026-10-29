# cardputer-adv

[![CI](https://github.com/tszarzynski/cardputer-adv/actions/workflows/ci.yml/badge.svg)](https://github.com/tszarzynski/cardputer-adv/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cardputer-adv.svg)](https://crates.io/crates/cardputer-adv)
[![API docs](https://img.shields.io/badge/docs-GitHub%20Pages-blue)](https://tszarzynski.github.io/cardputer-adv/)

Board support for the [M5Stack Cardputer ADV](https://docs.m5stack.com/en/core/Cardputer-Adv) (ESP32-S3) on [esp-hal](https://github.com/esp-rs/esp-hal) 1.x.

## Hardware

What the Cardputer ADV has, and what this crate covers so far.

| Part | Status | API |
|---|---|---|
| LCD, 1.14" ST7789V2, 240×135 | ✅ drawing directly, or a framebuffer sent by DMA; the controller's sleep | `display`, `frame_display`, `Frame`, `FrameDisplay::flush`, `FrameDisplay::set_asleep` |
| Backlight (GPIO38) | ✅ dimmable, 0–100% | `Backlight::set_brightness` |
| Keyboard, 56 keys (TCA8418) | ✅ Shift and Fn layers, presses and releases, interrupt-driven, wakes the chip from light sleep | `keyboard`, `keyboard_on`, `Keyboard::wait_for_inputs`, `Keyboard::wait_for_events`, `Keyboard::is_held`, `Keyboard::listen_for_wake` |
| Motion sensor (BMI270) | ✅ accelerometer, ±8 g at 100 Hz; gyroscope, ±2000 °/s at 100 Hz; low-power, normal and performance modes; temperature; ❌ motion interrupts (its INT pins don't reach the ESP32) | `imu`, `Imu::acceleration`, `Imu::set_gyroscope`, `Imu::calibrate_gyroscope`, `Imu::rotation`, `Imu::motion`, `Imu::set_mode`, `Imu::temperature` |
| Speaker (ES8311 codec, NS4150B amplifier) | ✅ playing samples at 44.1 kHz, tones, volume, output off for sleep | `speaker`, `Speaker::run`, `Tone`, `codec`, `Codec` |
| Microphone (through the ES8311, I2S in on GPIO46) | ✅ recording at 44.1 kHz, alongside the speaker; preamp gain | `audio`, `Audio::run`, `Codec::set_microphone`, `Codec::set_microphone_gain` |
| Battery (GPIO10) | ✅ voltage and estimated charge; charging can't be detected (no line for it) | `battery`, `Battery::voltage`, `Battery::percent` |
| Shared I2C bus (keyboard, codec, motion sensor) | ✅ | `i2c` |
| microSD slot (SPI: CS 12, MOSI 14, CLK 40, MISO 39) | ⚠️ FAT files through [embedded-sdmmc](https://crates.io/crates/embedded-sdmmc), with the `sd-card` feature; not yet tested on hardware | `sd_card`, `SdCard` |
| IR transmitter (GPIO44) | ✅ NEC codes on a 38 kHz carrier, and any other protocol's as raw pulses | `rmt`, `ir_transmitter`, `IrTransmitter::send_nec`, `IrTransmitter::send_raw` |
| RGB LED (WS2812 on GPIO21) | ✅ any colour; it's powered through GPIO38, so it's lit only while the backlight is | `rmt`, `rgb_led`, `RgbLed::set_color` |
| G0 button, after boot | ✅ debounced presses and releases; single, double and long clicks; wakes the chip from light sleep | `button`, `Button::wait_for_press`, `Button::wait_for_click` |
| Grove port (GPIO1, 2) and EXT header | not board-specific: use esp-hal directly | |

## API

- `display(DisplayPins)` sets up the 1.14" ST7789V2 LCD and turns the backlight on. You get it back as a 240×135 RGB565 [embedded-graphics](https://docs.rs/embedded-graphics) `DrawTarget`, in landscape with the keyboard below, and a `Backlight`. It draws straight to the panel, so each shape shows as it's drawn. `Backlight::set_brightness` dims it with 256 Hz PWM on LEDC (as M5GFX does; the LED driver doesn't follow fast PWM); the RGB LED, on the same pin, dims too.
- `frame_display(DisplayPins, dma)` sets up the same LCD to be fed from a `Frame`: a 240×135 RGB565 framebuffer (64.8 kB, kept in a static) that you draw into with embedded-graphics. `FrameDisplay::flush(&mut frame).await` then sends it by DMA, so other tasks run meanwhile.
  - Only pixels whose colour changed are sent: each run of changed rows goes as one rectangle, as wide as its changes. Redrawing the whole frame every time is fine.
  - A whole frame takes 13 ms at 40 MHz. Drawing straight to the panel through `display` sends a frame in about 28 ms, converting pixel by pixel.
  - `FrameDisplay::set_asleep(true).await` puts the panel's controller to sleep (a few µA instead of a few mA) with the picture kept in its memory; `set_asleep(false)` wakes it. Turn the backlight off first.
- `keyboard(KeyboardPins).await` sets up the 56-key TCA8418 keyboard on I2C, with Shift and Fn layers (`keys::KeyInput`; the layout is [cardputer-adv-keyboard](https://crates.io/crates/cardputer-adv-keyboard)'s). Fn + `;` `.` `,` `/` gives the arrow keys.
- `Keyboard::wait_for_inputs().await` sleeps until the TCA8418 pulls its interrupt line (GPIO11) low, then returns the keys pressed.
  - It waits for the low level, not an edge, so events that arrive while you're busy aren't lost.
  - A 1 s fallback poll covers a missed interrupt; `missed_interrupts()` counts how often that poll was needed.
  - Cancelling it (in a `select`) loses no key it has read: the next call returns it.
  - `Keyboard::is_held(input)` says whether a key that types `input` is down, with or without Shift and Fn (`Char('a')` and `Char('A')` are both the A key, `Arrow(Up)` the `;` key): for games, and chords such as Ctrl + C. It's as of the last event read, and `wait_for_inputs` reads them as they come while it's waiting, releases too.
  - `Keyboard::wait_for_events().await` waits the same way, and returns keys coming up as well as going down: `KeyEvent::Pressed(input)` and `KeyEvent::Released(input)`, in order. A release says what its press typed, even if Shift or Fn changed in between: for games, where letting go of a key matters.
  - `KeyInput::char()` is the character a key puts in text (Enter is `'\n'`, Tab `'\t'`); `None` for the keys that edit, move or modify. `KeyInput` displays as the key's name (`A`, `Space`, `Esc`, `Up`, `Ctrl`), for labels and hints.
  - `Keyboard::listen_for_wake()`, just before esp-hal's `LowPower::sleep_light`, makes a key press end the light sleep; `wait_for_inputs` then reads the key.
- `imu(&I2cBus).await` finds the BMI270 motion sensor (at 0x68 or 0x69), uploads its configuration (Bosch's, from [bmi2](https://crates.io/crates/bmi2)) and turns on its accelerometer at ±8 g and 100 Hz. `Imu::acceleration().await` reads it in milli-g.
- `speaker(SpeakerPins)` starts a 16-bit, 44.1 kHz I2S stream over DMA to the ES8311 codec, playing silence. `codec(&I2cBus).await` then turns on the codec's DAC path to the NS4150B amplifier and returns a `Codec`, whose `set_volume(percent).await` sets its level. `Speaker::run(fill).await` keeps the stream fed with the mono samples your closure writes.
  - The buffer holds 23 ms. If `fill` falls further behind, the stream restarts on silence, so the sound skips rather than stops.
  - `Codec::set_output(false)` powers the codec's DAC and its path to the amplifier down, and `set_output(true)` brings them back. Turn it off before the I2S clock stops (in a light sleep): the codec clocks itself from the stream, and the clock stopping and starting clicks in the speaker.
  - `Tone::new(Hertz(880), Duration::from_millis(100))` is a beep (embassy-time's `Duration`, or `core`'s): a square wave (or a triangle, `with_waveform(Waveform::Triangle)`) at a level (`with_level(percent)`, half the full scale unless set), faded in and out over 2 ms so it doesn't click. `tone.fill(samples)` writes its samples and then silence, so `speaker.run(|samples| beep.fill(samples))` plays it; it's an iterator of samples too, to mix.
  - To avoid skips while anything else holds the CPU for longer (drawing a frame straight to the panel takes about 30 ms), run the speaker on a higher-priority executor, such as esp-rtos's `InterruptExecutor`.
- `i2c(I2cPins)` sets up the I2C bus that the keyboard, codec and motion sensor share. Pass it to `keyboard_on`, `imu` and `codec`. `keyboard(KeyboardPins)` sets the bus up itself, for when the keyboard is all you need.
- `audio(AudioPins)` starts the speaker's stream and the microphone's together, on the same I2S clocks, and returns an `Audio`. Once `codec` has set the codec up, `Codec::set_microphone(true)` turns its ADC on, and `Codec::set_microphone_gain(db)` sets its preamp (0–30 dB, in 3 dB steps). `Audio::run(fill, take).await` plays what `fill` writes and hands what's heard to `take`, up to 256 mono samples at a time; the two run together because the microphone runs on the speaker's clocks. A stream that falls more than 23 ms behind restarts, so its sound has a gap.
- `Imu::set_gyroscope(true).await` turns the BMI270's gyroscope on (±2000 °/s at 100 Hz; it draws about 0.9 mA, so `imu` leaves it off), and `Imu::rotation().await` reads it in milli-degrees a second. Uncalibrated it can read several degrees a second at rest (from 0.5 to 9 °/s in different runs on the ADV tested); `Imu::calibrate_gyroscope().await`, with the Cardputer lying still for about 0.4 s, measures that offset and takes it off from then on.
- `Imu::set_mode(ImuMode).await` trades the motion sensor's current for noise, in the BMI270 datasheet's power modes. `imu` starts in `ImuMode::Performance`: the accelerometer draws 210 µA, 970 µA with the gyroscope. `ImuMode::Normal` drops the gyroscope's low-noise mode (685 µA with it). `ImuMode::LowPower` samples in bursts of 4, averaged: the accelerometer draws down to a few µA, both down to 420 µA, and the readings are noisier. That's enough for tilt and shakes, and worth it on battery: at 210 µA, the accelerometer alone is about a fifth of a light sleep's current. Calibrate the gyroscope again after changing mode.
- `Imu::motion().await` reads the acceleration and the rotation rate together (`Motion { acceleration, rotation }`), in one transfer: both of the same moment, for combining them, and half the bus traffic of reading them apart.
- `Imu::temperature().await` reads the BMI270's temperature (`Millicelsius`, shown as `27.4 °C`): the board's, a few degrees above the room's while it runs. It's measured every 1.28 s.
- `rmt(RMT)` sets up the RMT peripheral for the RGB LED and the IR transmitter; give each one of its transmit channels. `rgb_led(channel, GPIO21)` returns an `RgbLed`, and `RgbLed::set_color(Rgb888).await` sets it (black is off). It's powered through the backlight's switch, so it's lit only while the backlight is, and dims with it.
- `ir_transmitter(channel, GPIO44)` returns an `IrTransmitter`, which sends on a 38 kHz carrier. NEC codes: `send_nec(NecCode { address, command })`, `send_nec_extended(ExtendedNecCode { … })` for a 16-bit address, and `send_nec_repeat` for a held button. Any other protocol's (Sony, RC5, Samsung, an air conditioner's), or timings recorded from a remote: `send_raw(&[Pulse { mark_us, space_us }, …])`, each the carrier on for `mark_us` µs and then off for `space_us` µs, up to `IrTransmitter::MAX_PULSES` (128) pulses.
- `button(GPIO0)` returns the G0 `Button`: `is_pressed()`, `wait_for_press().await` and `wait_for_release().await` (debounced), and `listen_for_wake()` to wake the chip from light sleep, as `Keyboard::listen_for_wake` does.
  - `wait_for_click().await` tells a press apart, giving the one button three jobs: `Click::Single`, `Click::Double` (two presses within 300 ms), or `Click::Long` (held for half a second, told while it's still held). A single click is told 300 ms after its release, once no second press has come.
- `sd_card(SdCardPins)`, with the `sd-card` feature, sets up the microSD card's own SPI bus (SPI3) and returns an `SdCard`, an [embedded-sdmmc](https://crates.io/crates/embedded-sdmmc) `BlockDevice`. Read and write FAT files through `embedded_sdmmc::VolumeManager`; the crate re-exports `embedded_sdmmc`, so the versions match. The card is started on first use (at 400 kHz, then run at 20 MHz), and again after a failed access, so it can be inserted or swapped while the app runs. It's blocking, so a long read holds up its executor.
- Each part's pins come out of esp-hal's `Peripherals` with a macro named after it: `display_pins!(p)`, `i2c_pins!(p)`, `keyboard_pins!(p)`, `speaker_pins!(p)`, `audio_pins!(p)`, `battery_pins!(p)` and `sd_card_pins!(p)`. Only those fields move, so the rest of `p` stays usable. The `…Pins` structs can still be written out by hand.
- `battery(BatteryPins)` measures the battery voltage (`Battery::voltage()`, in `Millivolts`) on GPIO10 through the board's 2:1 divider, as M5Stack's M5Unified does, and `Battery::percent()` the charge left (a `Percent`), by M5Unified's estimate (3.30 V empty, 4.10 V full). The board can't tell whether it's charging: on USB with the power switch on, it reads the charging voltage.

```rust
use cardputer_adv::{display_pins, keyboard_pins};
use embedded_graphics::{pixelcolor::Rgb565, prelude::*};

let (mut display, mut backlight) = cardputer_adv::display(display_pins!(p))?;
let mut keyboard = cardputer_adv::keyboard(keyboard_pins!(p)).await?;
display.clear(Rgb565::BLACK).expect("LCD"); // mipidsi's own error type
loop {
    for key in keyboard.wait_for_inputs().await? { /* … */ }
}
```

With the motion sensor and the speaker, set the shared bus up first:

```rust
use cardputer_adv::{Acceleration, Percent, i2c_pins, speaker_pins};

let bus = cardputer_adv::i2c(i2c_pins!(p))?;
let mut keyboard = cardputer_adv::keyboard_on(bus, p.GPIO11).await?;
let mut imu = cardputer_adv::imu(bus).await?;
let Acceleration { x, y, z } = imu.acceleration().await?; // milli-g

// In a task (ideally on a higher-priority executor):
let mut speaker = cardputer_adv::speaker(speaker_pins!(p))?;
// …and once it's running (the codec clocks itself from the stream):
let mut codec = cardputer_adv::codec(bus).await?;
codec.set_volume(Percent::new(60)).await?;
let Err(error) = speaker.run(|samples| samples.fill(0)).await; // runs until it fails
```

The microphone, the RGB LED and the IR transmitter:

```rust
use cardputer_adv::{Decibels, NecCode, audio_pins};
use embedded_graphics::pixelcolor::{Rgb888, RgbColor};

let mut audio = cardputer_adv::audio(audio_pins!(p))?;
let mut codec = cardputer_adv::codec(bus).await?;
codec.set_microphone_gain(Decibels(24)).await?;
codec.set_microphone(true).await?;
// In a task: silence out, 44.1 kHz mono in.
let Err(error) = audio.run(|out| out.fill(0), |heard| { /* … */ }).await;

let rmt = cardputer_adv::rmt(p.RMT)?;
let mut led = cardputer_adv::rgb_led(rmt.channel0, p.GPIO21)?;
let mut ir = cardputer_adv::ir_transmitter(rmt.channel1, p.GPIO44)?;
led.set_color(Rgb888::GREEN).await?;
ir.send_nec(NecCode { address: 0x00, command: 0x45 }).await?;
```

With a framebuffer, sent by DMA:

```rust
use cardputer_adv::Frame;
use static_cell::ConstStaticCell;

static FRAME: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());

let (mut display, _backlight) = cardputer_adv::frame_display(display_pins!(p), p.DMA_CH1)?;
let frame = FRAME.take();
loop {
    let Ok(()) = frame.clear(Rgb565::BLACK); // it can't fail
    // …draw the rest…
    display.flush(frame).await?; // only what changed
}
```

## Requirements

- esp-hal 1.2, with its `unstable` feature (LEDC, I2S, RMT and light sleep are behind it), on an ESP32-S3.
- The `sd-card` feature, for the microSD card. It's off by default, so apps without a card don't build embedded-sdmmc.
- The `defmt` feature, for `defmt::Format` on `Error`, if your app logs with defmt.
- An embassy-time driver, for the async parts (the keyboard and the motion sensor, from setting them up on, `FrameDisplay::flush`, `Speaker::run`): esp-rtos's, say.
- `Display` is a [mipidsi](https://crates.io/crates/mipidsi) type; the crate re-exports `mipidsi`, so you can name its types without depending on it yourself.

## Sleeping while the screen is dark

The ESP32-S3 can light-sleep (about 1 mA) and carry on where it left off, woken by a key. In this order, with esp-hal's `LowPower`:

```rust
use esp_hal::{
    rtc_cntl::sleep::{LowPower, RtcSleepConfig},
    time::{Duration, Instant},
};

let mut low_power = LowPower::new(p.LPWR);

// Going dark:
backlight.set_brightness(Percent::ZERO)?;
display.set_asleep(true).await?; // the panel controller sleeps, keeping its picture
codec.set_output(false).await?;  // before the I2S clock stops, or the speaker clicks

// Each time round while dark:
keyboard.listen_for_wake();      // a key press ends the sleep
low_power.set_wakeup_deadline(Instant::now() + Duration::from_millis(250));
low_power.sleep_light(RtcSleepConfig::default()); // returns at the key, or the deadline
// …look at the motion sensor, say, then read keys with wait_for_inputs as usual.

// Lighting up again:
display.set_asleep(false).await?;
codec.set_output(true).await?;
backlight.set_brightness(Percent::FULL)?;
```

While the chip sleeps its USB is off (no log, no flashing until it wakes), and the embassy clock stops. The speaker's stream runs dry and restarts on silence by itself.

## Pins

| Function | GPIO |
|---|---|
| LCD SCK / MOSI / CS / DC / RST | 36 / 35 / 37 / 34 / 33 |
| LCD backlight, PWM (also powers the RGB LED) | 38 |
| Battery voltage (ADC1, 2:1 divider) | 10 |
| I2C SDA / SCL (keyboard, ES8311 codec, BMI270 IMU) | 8 / 9 |
| Keyboard interrupt | 11 |
| I2S BCLK / WS / DOUT / DIN, the ES8311 (no MCLK: it clocks from BCLK) | 41 / 43 / 42 / 46 |
| G0 button | 0 |
| RGB LED (WS2812) | 21 |
| IR LED | 44 |
| microSD SCK / MOSI / MISO / CS (SPI3) | 40 / 14 / 39 / 12 |

The I2C bus is async, shared through an async mutex (embassy-sync's, with embassy-embedded-hal's `I2cDevice`s), so the keyboard, the codec and the motion sensor can be used from any executor or core. A task waiting for the bus or a transfer lets the others run, and no transfer holds interrupts off, not even the motion sensor's 8 kB start-up.

## Status

The display settings (panel offset 52/40, 90° rotation, colour inversion) come from working ADV firmware ([iwan-inc/cardputer-rust](https://github.com/iwan-inc/cardputer-rust)). The display and the keyboard, including its interrupt line, work on a Cardputer ADV (tested October 2026). So do the motion sensor and the speaker. `imu` tries the BMI270's two addresses, 0x68 then 0x69, because sources disagree on which one the ADV uses (espp says 0x68, oxstack 0x69). The ES8311 sequence is M5Unified's Cardputer ADV speaker enable, as [espp](https://github.com/esp-cpp/espp/tree/main/components/m5stack-cardputer)'s driver uses it. The battery reading is new and not yet checked against a multimeter. The pins for the microphone, the microSD card, the IR LED, the RGB LED and G0 are M5Unified's for the ADV, and the microphone's codec sequence is its microphone enable. The microphone, the gyroscope, G0, the RGB LED and IR transmission work on the ADV too (tested October 2026; the IR LED's light itself not yet seen). The microSD card hasn't been tested on hardware yet. `examples/hardware.rs` (`just hardware`) tries each of these parts, showing its steps and readings on the screen.

## Documentation

docs.rs can't build for Xtensa, so the [API docs](https://tszarzynski.github.io/cardputer-adv/) are on GitHub Pages instead, built by CI from `main`. To build them locally, run `just doc`.

## Licence

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT licence ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 licence, shall be dual licensed as above, without any additional terms or conditions.
