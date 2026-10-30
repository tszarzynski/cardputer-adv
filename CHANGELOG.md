# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.4.3] - 2026-10-30

- README: GIFs of the hardware example and of backgammon, made with screenshots from the device, and a Built with cardputer-adv section.
- Screenshots: `Frame::screenshot()` gives what the screen shows as one line of text (`Screenshot`, starting `SCREENSHOT_MARKER`) for the app to log (sound stalls meanwhile: esp-println holds interrupts off while it prints), and `Mirror` copies drawing straight to the `Display` into a `Frame`, for apps without one. `tools/screenshots.py` turns a log of them into PNGs, and `just gif` into a GIF; `just screenshots` runs the hardware test with them on.

- `examples/hello.rs`, the smallest app: what you type shows on the screen.
- README: a Getting started section (the toolchain, and the `Cargo.toml`, `.cargo/config.toml` and `rust-toolchain.toml` for an app of your own); it says the crate is for the ADV only, and why; Status is a list of what's tested on hardware and what isn't.
- `Keyboard::wait_for_inputs` is about 3 kB smaller in flash: one wait for keys or the next repeat, rather than two copies of the keyboard read.

## [0.4.2] - 2026-10-29

- Key auto-repeat: `Keyboard::set_repeat(Some(Repeat::default()))` repeats a held key in `wait_for_inputs`, after 400 ms and then 20 times a second (`Repeat::new` for other timings). Off by default.
- `Imu::set_accelerometer_range` (`AccelerometerRange`, ±2 to ±16 g) and `Imu::set_gyroscope_range` (`GyroscopeRange`, ±125 to ±2000 °/s), with getters. Readings keep their units, and a gyroscope calibration carries over. Each waits 80 ms, so the next reading is in the new range: the sensor holds its last reading, in the old range, for up to 60 ms after a change.
- The hardware example repeats held keys, steps the accelerometer's range on G0's double clicks and the gyroscope's on long presses, and counts both.

## [0.4.1] - 2026-10-29

- `Keyboard::wait_for_events` returns keys coming up as well as going down (`keys::KeyEvent`), for games; a release says what its press typed. `wait_for_inputs` is built on it, and returns the presses as before.
- `KeyInput::char()`, the character a key puts in text, and `Display` for `KeyInput`, `Arrow` and `Modifier`: the key's name.
- `Button::wait_for_release`, and `Button::wait_for_click`, which tells a single click, a double click and a long press apart (`Click`).
- `Tone`, a beep for `Speaker::run` and `Audio::run`: a square or triangle wave (`Waveform`) at a frequency (`Hertz`), for a time, faded in and out. Its duration can be embassy-time's `Duration` as well as `core`'s.
- `Imu::temperature`, in `Millicelsius`.
- `Imu::motion`: the acceleration and the rotation rate in one read (`Motion`), of the same moment, and half the bus traffic of reading them apart.
- The hardware example beeps each G0 click by its kind, logs keys' releases, and shows the temperature.
- The README says why the motion sensor has no interrupts: on the ADV, its INT1 and INT2 pins don't reach the ESP32. Tested on the device: neither one's level showed on any free GPIO, nor on the keyboard controller's spare inputs.
- `imu`'s docs give the time it takes as measured on the device, about 230 ms (they said its upload took about 50 ms).

## [0.4.0] - 2026-10-29

- Breaking: the I2C bus is async. `I2cBus` is an embassy-sync async mutex and `I2cDevice` embassy-embedded-hal's async device, so a task waiting for the bus lets others run, and no transfer holds interrupts off (they held a critical section, up to 3 ms at a time while the motion sensor started).
  - Async: `keyboard`, `keyboard_on`, `imu` and `codec`; `Imu::acceleration`, `rotation` and `set_gyroscope`; `Codec::set_volume`, `set_output`, `set_microphone` and `set_microphone_gain`.
  - The keyboard and the motion sensor have drivers of their own, for the async bus. `keys` is this crate's (`KeyInput`, `Arrow` and `Modifier`, as before) instead of a re-export of cardputer-adv-keyboard, which isn't a dependency any more; `Keyboard::driver_mut` is gone. bmi2 is now only where the BMI270's configuration comes from.
  - `KeyInput`, `Arrow` and `Modifier` are `#[non_exhaustive]`, so keys can be added without a breaking change: a `match` on them needs a `_` arm.
- `Keyboard::wait_for_inputs` keeps the keys it has read when it's cancelled, and returns them next time.
- `Imu::set_mode` sets the motion sensor's power mode (`ImuMode`: `LowPower`, `Normal` or `Performance`, the datasheet's), trading current for noise. `imu` still starts in `Performance`, as before; `LowPower` takes the accelerometer from 210 µA down to a few µA.
- `Keyboard::is_held` says whether a key is held down (for games, and chords such as Ctrl + C), by what it types with or without Shift and Fn. The keys `wait_for_inputs` returns no longer keep the keyboard borrowed, so it can be asked while going through them.
- README: the framebuffer and sleep examples compile (`Frame::clear` can't fail, so no `?`; the sleep example imports `Duration` and `Instant`).
- `examples/flush.rs` (`just flush`) times `FrameDisplay::flush` on a Cardputer for changed areas of different shapes, and logs it.
- CI: `just semver` checks the public API against the last release on crates.io with cargo-semver-checks, so a breaking change can't go out without a new version.
- Host tests: the keyboard's controller (`tca8418`, apart from its interrupt line and the waiting) runs against a fake TCA8418, covering a read that's cancelled partway, a failed read or flag clear, events left in the FIFO, and the missed-interrupt count.
- The README's examples and the crate docs' quick start are compiled by `just readme` (part of `just check`, so CI), so they can't go stale again.

## [0.3.0] - 2026-10-28

- Breaking: units and codes have types of their own, so they can't be mixed up.
  - `Percent` (0–100; `Percent::new` caps it at 100) for `Backlight::set_brightness` (was `set_percent`), `Codec::set_volume`, `Battery::percent` and `battery_percent`.
  - `Millivolts` for `Battery::voltage` (was `millivolts`) and `battery_percent`; `Decibels` for `Codec::set_microphone_gain`.
  - `Acceleration` and `Rotation`, with `x`, `y` and `z` fields, for `Imu::acceleration` and `Imu::rotation` (were `[i32; 3]`).
  - `NecCode` and `ExtendedNecCode` for `IrTransmitter::send_nec` and `send_nec_extended` (were two arguments each), and `Pulse { mark_us, space_us }` for `send_raw` (was `(u16, u16)`).
- `FrameDisplay::flush` and `set_asleep` deselect the panel even when cancelled mid-transfer, so the next transfer starts cleanly.
- `FrameDisplay::flush` sends a run of changed rows nearly as wide as the screen (about 190 pixels, for a tall run) as whole rows, in one transfer instead of one a row: 135 rows 200 wide took longer than the whole screen.

## [0.2.1] - 2026-10-27

- `IrTransmitter::send_raw` sends any remote's code as `(mark, space)` pulses in µs, up to `IrTransmitter::MAX_PULSES` (128); `MAX_DURATION` is the longest mark or space.
- Host tests: the pure logic (the unit conversions, the LED's and IR codes' pulses, the framebuffer's change tracking) is in modules without esp-hal, which `host-tests/` builds and tests on the host (`just test`, and in CI).
- CI: `actions/checkout@v5`, `just` from `taiki-e/install-action`, and the runner pinned to Ubuntu 24.04.

## [0.2.0] - 2026-10-27

- The microphone: `audio` starts the speaker's and the microphone's I2S streams together (`AudioPins`), `Audio::run` plays and hands over what's heard, and `Codec::set_microphone` and `Codec::set_microphone_gain` turn the codec's ADC on and set its preamp.
- The gyroscope: `Imu::set_gyroscope` and `Imu::rotation`, in milli-degrees a second, and `Imu::calibrate_gyroscope`, which takes off its offset at rest.
- The RGB LED and the IR transmitter, on the RMT (`rmt`, `RMT_FREQUENCY`): `rgb_led` and `RgbLed::set_color`; `ir_transmitter` and `IrTransmitter::send_nec`, `send_nec_extended` and `send_nec_repeat`.
- The G0 button: `button`, `Button::is_pressed`, `Button::wait_for_press` (debounced) and `Button::listen_for_wake`.
- The microSD card, with the `sd-card` feature: `sd_card` returns an `SdCard`, an embedded-sdmmc `BlockDevice` that starts the card on first use (and again after a failure, for a card inserted or swapped later); `embedded_sdmmc` is re-exported.
- A macro per part takes its pins out of esp-hal's `Peripherals`: `display_pins!`, `i2c_pins!`, `keyboard_pins!`, `speaker_pins!`, `audio_pins!`, `battery_pins!` and `sd_card_pins!`.
- Breaking: `enable_codec` is now `codec`, `KeyboardPins` holds the bus's `I2cPins` (`i2c`) instead of its three fields, `Error::Spi` is `Error::DisplaySpi`, `Keyboard::wait_inputs` is `wait_for_inputs` (as `Button::wait_for_press`), and `Keyboard::driver` is `driver_mut`.
- The `defmt` feature: `defmt::Format` for `Error`.
- API docs on GitHub Pages, built by CI (fmt, clippy, the builds, the docs and `cargo package`).
- Every public type implements `Debug`.
- `examples/hardware.rs` tries each part on a Cardputer (`just hardware`).

## [0.1.0] - 2026-10-26

- First version: display, keyboard (interrupt-driven `wait_inputs`, with a fallback poll), the backlight (`Backlight::set_percent`, 256 Hz PWM) and the battery voltage (`battery`).
- The shared I2C bus (`i2c`, `keyboard_on`); `keyboard` still sets it up for the keyboard alone.
- The BMI270 accelerometer (`imu`, `Imu::acceleration` in milli-g).
- The speaker: an I2S DMA stream (`speaker`, `Speaker::run`), the ES8311 codec's DAC path (`enable_codec`, which returns a `Codec`) and its level (`Codec::set_volume`). The stream restarts on silence if it runs dry.
- A framebuffer sent by DMA (`frame_display`, `Frame`, `FrameDisplay::flush`), sending only the pixels that changed.
- The I2C bus is a critical-section mutex rather than a `RefCell`, so its devices can be used from any executor or core.
- `keyboard_on` (and `keyboard`) retry setting up the TCA8418, which can miss the first transfers right after a flash or a reset.
- `Error` documents its variants and implements `Display` and `core::error::Error`; it's `#[non_exhaustive]`.
- For low power: `FrameDisplay::set_asleep` (the panel controller's sleep), `Keyboard::listen_for_wake` (a key ends an esp-hal light sleep) and `Codec::set_output` (the codec's output off and on, so a stopped I2S clock doesn't click).
- `Battery::percent` and `battery_percent`: the charge left, by M5Unified's estimate.
- `mipidsi` is re-exported, as `Display` is one of its types.
