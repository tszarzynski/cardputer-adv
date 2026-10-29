//! The BMI270 motion sensor's registers, and the parts of talking to it
//! that don't need the bus: the configuration upload's chunks and the
//! readings' layout. The sequence is the datasheet's, as the
//! [bmi2](https://crates.io/crates/bmi2) crate follows it.

/// What its `CHIP_ID` register reads.
pub const CHIP_ID: u8 = 0x24;

/// The addresses it can be at: 0x68 with SDO low, 0x69 high.
pub const ADDRESSES: [u8; 2] = [0x68, 0x69];

/// Its registers.
pub mod register {
    pub const CHIP_ID: u8 = 0x00;
    /// The accelerometer's x, y and z, each 16-bit little-endian.
    pub const ACC_DATA: u8 = 0x0C;
    /// The gyroscope's x, y and z, each 16-bit little-endian.
    pub const GYR_DATA: u8 = 0x12;
    pub const INTERNAL_STATUS: u8 = 0x21;
    pub const ACC_CONF: u8 = 0x40;
    pub const ACC_RANGE: u8 = 0x41;
    pub const GYR_CONF: u8 = 0x42;
    pub const GYR_RANGE: u8 = 0x43;
    pub const INIT_CTRL: u8 = 0x59;
    /// The upload's word address, low nibble here and the rest in the next
    /// register.
    pub const INIT_ADDR_0: u8 = 0x5B;
    pub const INIT_DATA: u8 = 0x5E;
    pub const PWR_CONF: u8 = 0x7C;
    pub const PWR_CTRL: u8 = 0x7D;
    pub const CMD: u8 = 0x7E;
}

/// `CMD`: a soft reset.
pub const SOFT_RESET: u8 = 0xB6;
/// `PWR_CONF`: advanced power save.
pub const ADVANCED_POWER_SAVE: u8 = 1 << 0;
/// `PWR_CTRL`: the gyroscope on.
pub const GYROSCOPE_ON: u8 = 1 << 1;
/// `PWR_CTRL`: the accelerometer on.
pub const ACCELEROMETER_ON: u8 = 1 << 2;
/// `INTERNAL_STATUS`: the configuration was taken.
pub const INITIALISED: u8 = 1 << 0;

/// `ACC_RANGE`: ±8 g.
pub const ACC_RANGE_8G: u8 = 0x02;
/// `GYR_RANGE`: ±2000 °/s, and the same for its OIS output.
pub const GYR_RANGE_2000: u8 = 1 << 3;

/// `ACC_CONF` and `GYR_CONF`: 100 Hz, and their bandwidth field at 2: the
/// normal filter, or in low-power mode, averaging 4 samples.
const CONF_100HZ: u8 = 0x08 | 0x02 << 4;
/// `ACC_CONF` and `GYR_CONF`: the filter for continuous sampling, not the
/// low-power duty cycle.
const FILTER_PERF: u8 = 1 << 7;
/// `GYR_CONF`: the gyroscope's low-noise mode.
const NOISE_PERF: u8 = 1 << 6;

/// The motion sensor's power modes, as its datasheet has them (Table 6),
/// trading current for noise. The currents are typical, for the sensor
/// alone; with advanced power save on, which [`imu`] leaves it.
///
/// [`imu`]: crate::imu
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum ImuMode {
    /// Each sensor wakes for 4 samples a reading and averages them: the
    /// accelerometer alone draws down to a few µA, both down to 420 µA. The
    /// readings are noisier, and filtered differently.
    LowPower,
    /// Continuous sampling: the accelerometer draws 210 µA, both 685 µA.
    Normal,
    /// As [`Normal`](Self::Normal), with the gyroscope's low-noise mode on:
    /// both draw 970 µA. What [`imu`] starts in.
    ///
    /// [`imu`]: crate::imu
    #[default]
    Performance,
}

impl ImuMode {
    /// `ACC_CONF` in this mode.
    pub(crate) const fn acc_conf(self) -> u8 {
        match self {
            Self::LowPower => CONF_100HZ,
            Self::Normal | Self::Performance => CONF_100HZ | FILTER_PERF,
        }
    }

    /// `GYR_CONF` in this mode.
    pub(crate) const fn gyr_conf(self) -> u8 {
        match self {
            Self::LowPower => CONF_100HZ,
            Self::Normal => CONF_100HZ | FILTER_PERF,
            Self::Performance => CONF_100HZ | FILTER_PERF | NOISE_PERF,
        }
    }
}

/// The most configuration bytes one write carries: a whole number of
/// 16-bit words, with the register address making it 127 bytes.
pub const UPLOAD_CHUNK: usize = 126;

/// The configuration's chunks, each with the `INIT_ADDR` registers' two
/// bytes that point at it (its offset in 16-bit words).
#[expect(
    clippy::cast_possible_truncation,
    reason = "the address is 12 bits: the configuration is 8 kB, 4096 words"
)]
pub fn upload_chunks(config: &[u8]) -> impl Iterator<Item = ([u8; 2], &[u8])> {
    config
        .chunks(UPLOAD_CHUNK)
        .enumerate()
        .map(|(index, chunk)| {
            let word = index * UPLOAD_CHUNK / 2;
            let address = [(word & 0x0F) as u8, (word >> 4) as u8];
            (address, chunk)
        })
}

/// A reading's three axes, from its six bytes.
pub fn axes(data: [u8; 6]) -> [i32; 3] {
    let [x0, x1, y0, y1, z0, z1] = data;
    [[x0, x1], [y0, y1], [z0, z1]].map(|bytes| i32::from(i16::from_le_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_configuration_goes_in_word_addressed_chunks() {
        // Each byte different from its neighbours, so a chunk from the
        // wrong place shows.
        let config: [u8; 8192] = core::array::from_fn(|index| index.to_le_bytes()[0]);
        let mut chunks = upload_chunks(&config);
        assert_eq!(chunks.next(), Some(([0, 0], &config[..UPLOAD_CHUNK])));
        // 63 words in: 0x3F, low nibble then the rest.
        assert_eq!(
            chunks.next(),
            Some(([0x0F, 0x03], &config[UPLOAD_CHUNK..2 * UPLOAD_CHUNK]))
        );
        assert_eq!(upload_chunks(&config).count(), 66);
        // 4095 words in, the last two bytes.
        assert_eq!(chunks.last(), Some(([0x0F, 0xFF], &config[8190..])));
    }

    #[test]
    fn a_reading_is_three_little_endian_axes() {
        assert_eq!(
            axes([0x00, 0x10, 0xFF, 0xFF, 0x00, 0x80]),
            [4096, -1, -32768]
        );
    }

    #[test]
    fn the_settings_are_the_datasheets() {
        // Its examples: normal power mode, ACC_CONF 0xA8; performance mode,
        // GYR_CONF 0xE8.
        assert_eq!(ImuMode::Performance.acc_conf(), 0xA8);
        assert_eq!(ImuMode::Performance.gyr_conf(), 0xE8);
        assert_eq!(ImuMode::Normal.acc_conf(), 0xA8);
        assert_eq!(ImuMode::Normal.gyr_conf(), 0xA8);
        // Low power: no performance bits, 100 Hz, averaging 4.
        assert_eq!(ImuMode::LowPower.acc_conf(), 0x28);
        assert_eq!(ImuMode::LowPower.gyr_conf(), 0x28);
        assert_eq!(GYR_RANGE_2000, 0x08);
    }
}
