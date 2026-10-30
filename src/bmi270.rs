//! The BMI270 motion sensor's registers, and the parts of talking to it
//! that don't need the bus: the configuration upload's chunks and the
//! readings' layout. The sequence is the datasheet's, as the
//! [bmi2](https://crates.io/crates/bmi2) crate follows it.

use core::time::Duration;

use crate::units::Hertz;

/// What its `CHIP_ID` register reads.
pub const CHIP_ID: u8 = 0x24;

/// The addresses it can be at: 0x68 with SDO low, 0x69 high.
pub const ADDRESSES: [u8; 2] = [0x68, 0x69];

/// Its registers.
pub mod register {
    pub const CHIP_ID: u8 = 0x00;
    /// The accelerometer's x, y and z, each 16-bit little-endian.
    pub const ACC_DATA: u8 = 0x0C;
    /// The gyroscope's x, y and z, each 16-bit little-endian, right after
    /// the accelerometer's.
    pub const GYR_DATA: u8 = 0x12;
    pub const INTERNAL_STATUS: u8 = 0x21;
    /// The temperature, 16-bit little-endian.
    pub const TEMPERATURE: u8 = 0x22;
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

/// `GYR_RANGE`: its OIS output at ±2000 °/s, whatever the main range.
const OIS_RANGE_2000: u8 = 1 << 3;

/// The accelerometer's range: the most it reads, against how finely. Each
/// reading is 16 bits across the range, so ±2 g reads to 0.06 mg and
/// ±16 g to 0.5 mg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum AccelerometerRange {
    /// ±2 g: for tilt, where gravity is all there is.
    G2,
    /// ±4 g.
    G4,
    /// ±8 g: tilt and shakes. What [`imu`] starts at.
    ///
    /// [`imu`]: crate::imu
    #[default]
    G8,
    /// ±16 g: for hard knocks.
    G16,
}

impl AccelerometerRange {
    /// `ACC_RANGE` at this range.
    pub(crate) const fn register(self) -> u8 {
        match self {
            Self::G2 => 0,
            Self::G4 => 1,
            Self::G8 => 2,
            Self::G16 => 3,
        }
    }

    /// The range's end, in g.
    pub(crate) const fn full_scale(self) -> i32 {
        match self {
            Self::G2 => 2,
            Self::G4 => 4,
            Self::G8 => 8,
            Self::G16 => 16,
        }
    }
}

/// The gyroscope's range: the fastest turn it reads, against how finely.
/// Each reading is 16 bits across the range, so ±125 °/s reads to
/// 0.004 °/s and ±2000 °/s to 0.06 °/s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum GyroscopeRange {
    /// ±125 °/s: slow, careful turns.
    Dps125,
    /// ±250 °/s.
    Dps250,
    /// ±500 °/s.
    Dps500,
    /// ±1000 °/s.
    Dps1000,
    /// ±2000 °/s: a flick of the wrist. What [`imu`] starts at.
    ///
    /// [`imu`]: crate::imu
    #[default]
    Dps2000,
}

impl GyroscopeRange {
    /// `GYR_RANGE` at this range.
    pub(crate) const fn register(self) -> u8 {
        OIS_RANGE_2000
            | match self {
                Self::Dps2000 => 0,
                Self::Dps1000 => 1,
                Self::Dps500 => 2,
                Self::Dps250 => 3,
                Self::Dps125 => 4,
            }
    }

    /// The range's end, in degrees a second.
    pub(crate) const fn full_scale(self) -> i32 {
        match self {
            Self::Dps125 => 125,
            Self::Dps250 => 250,
            Self::Dps500 => 500,
            Self::Dps1000 => 1000,
            Self::Dps2000 => 2000,
        }
    }
}

/// `ACC_CONF` and `GYR_CONF`: their bandwidth field at 2, the normal
/// filter, or in low-power mode, averaging 4 samples.
const BANDWIDTH: u8 = 0x02 << 4;
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
    /// Whether the sensor can sample at `rate` in this mode: in low power,
    /// each reading averages 4 samples, which caps it at 200 Hz (asked for
    /// 400, it gave 200, measured on the ADV).
    pub(crate) const fn supports(self, rate: ImuRate) -> bool {
        !matches!(self, Self::LowPower) || rate.hertz().0 <= 200
    }

    /// `ACC_CONF` in this mode, at `rate`.
    pub(crate) const fn acc_conf(self, rate: ImuRate) -> u8 {
        let conf = rate.odr() | BANDWIDTH;
        match self {
            Self::LowPower => conf,
            Self::Normal | Self::Performance => conf | FILTER_PERF,
        }
    }

    /// `GYR_CONF` in this mode, at `rate`.
    pub(crate) const fn gyr_conf(self, rate: ImuRate) -> u8 {
        let conf = rate.odr() | BANDWIDTH;
        match self {
            Self::LowPower => conf,
            Self::Normal => conf | FILTER_PERF,
            Self::Performance => conf | FILTER_PERF | NOISE_PERF,
        }
    }
}

/// How often the motion sensor measures, for [`Imu::set_rate`].
///
/// The accelerometer and the gyroscope together. Faster catches quicker
/// movements (taps, vibration); slower draws less in
/// [`ImuMode::LowPower`], where the current follows the rate, and which
/// goes up to 200 Hz.
///
/// [`Imu::set_rate`]: crate::Imu::set_rate
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum ImuRate {
    /// 25 Hz.
    Hz25,
    /// 50 Hz.
    Hz50,
    /// 100 Hz, what [`imu`] starts at.
    ///
    /// [`imu`]: crate::imu
    #[default]
    Hz100,
    /// 200 Hz.
    Hz200,
    /// 400 Hz.
    Hz400,
    /// 800 Hz.
    Hz800,
    /// 1600 Hz, the accelerometer's fastest.
    Hz1600,
}

impl ImuRate {
    /// The rate, in hertz.
    #[must_use]
    pub const fn hertz(self) -> Hertz {
        Hertz(match self {
            Self::Hz25 => 25,
            Self::Hz50 => 50,
            Self::Hz100 => 100,
            Self::Hz200 => 200,
            Self::Hz400 => 400,
            Self::Hz800 => 800,
            Self::Hz1600 => 1600,
        })
    }

    /// The time between readings.
    #[must_use]
    pub const fn period(self) -> Duration {
        Duration::from_micros(1_000_000 / self.hertz().0 as u64)
    }

    /// The `ACC_CONF` and `GYR_CONF` rate field: 25 Hz is 6, and each step
    /// up doubles it.
    const fn odr(self) -> u8 {
        match self {
            Self::Hz25 => 0x06,
            Self::Hz50 => 0x07,
            Self::Hz100 => 0x08,
            Self::Hz200 => 0x09,
            Self::Hz400 => 0x0A,
            Self::Hz800 => 0x0B,
            Self::Hz1600 => 0x0C,
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
        // Its examples, at 100 Hz: normal power mode, ACC_CONF 0xA8;
        // performance mode, GYR_CONF 0xE8.
        let rate = ImuRate::Hz100;
        assert_eq!(ImuMode::Performance.acc_conf(rate), 0xA8);
        assert_eq!(ImuMode::Performance.gyr_conf(rate), 0xE8);
        assert_eq!(ImuMode::Normal.acc_conf(rate), 0xA8);
        assert_eq!(ImuMode::Normal.gyr_conf(rate), 0xA8);
        // Low power: no performance bits, averaging 4.
        assert_eq!(ImuMode::LowPower.acc_conf(rate), 0x28);
        assert_eq!(ImuMode::LowPower.gyr_conf(rate), 0x28);
    }

    #[test]
    fn the_rates_are_the_datasheets() {
        // 25 Hz is 0x06, 1600 Hz 0x0C, as the rate field takes them.
        assert_eq!(ImuMode::Normal.acc_conf(ImuRate::Hz25), 0xA6);
        assert_eq!(ImuMode::Performance.gyr_conf(ImuRate::Hz1600), 0xEC);
        assert_eq!(ImuMode::LowPower.acc_conf(ImuRate::Hz200), 0x29);
        assert_eq!(ImuRate::Hz200.period(), Duration::from_millis(5));
        assert_eq!(ImuRate::Hz1600.hertz(), Hertz(1600));
    }

    #[test]
    fn low_power_goes_up_to_200_hz() {
        assert!(ImuMode::LowPower.supports(ImuRate::Hz200));
        assert!(!ImuMode::LowPower.supports(ImuRate::Hz400));
        assert!(!ImuMode::LowPower.supports(ImuRate::Hz800));
        assert!(ImuMode::Normal.supports(ImuRate::Hz1600));
    }

    #[test]
    fn the_ranges_registers_are_the_datasheets() {
        // ACC_RANGE 0x02 is ±8 g, GYR_RANGE 0x08 ±2000 °/s (OIS too).
        assert_eq!(AccelerometerRange::default().register(), 0x02);
        assert_eq!(AccelerometerRange::G16.register(), 0x03);
        assert_eq!(GyroscopeRange::default().register(), 0x08);
        assert_eq!(GyroscopeRange::Dps125.register(), 0x0C);
    }
}
