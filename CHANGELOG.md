# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
