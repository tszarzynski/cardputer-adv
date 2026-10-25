# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

- First version: display, keyboard (interrupt-driven `wait_inputs`, with a fallback poll), the backlight (`Backlight::set_percent`, 256 Hz PWM) and the battery voltage (`battery`).
- The shared I2C bus (`i2c`, `keyboard_on`); `keyboard` still sets it up for the keyboard alone.
- The BMI270 accelerometer (`imu`, `Imu::acceleration` in milli-g).
- The speaker: an I2S DMA stream (`speaker`, `Speaker::run`), the ES8311 codec's DAC path (`enable_codec`) and its level (`set_volume`). The stream restarts on silence if it runs dry.
- A framebuffer sent by DMA (`frame_display`, `Frame`, `FrameDisplay::flush`), sending only the pixels that changed.
- The I2C bus is a critical-section mutex rather than a `RefCell`, so its devices can be used from any executor or core.
- `keyboard_on` (and `keyboard`) retry setting up the TCA8418, which can miss the first transfers right after a flash or a reset.
- `Error` documents its variants and implements `Display` and `core::error::Error`; it's `#[non_exhaustive]`.
