# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

- First version: display, keyboard (interrupt-driven `wait_inputs`, with a fallback poll).
- The shared I2C bus (`i2c`, `keyboard_on`); `keyboard` still sets it up for the keyboard alone.
- The BMI270 accelerometer (`imu`, `Imu::acceleration` in milli-g).
- The speaker: an I2S DMA stream (`speaker`, `Speaker::run`) and the ES8311 codec's DAC path (`enable_codec`).
