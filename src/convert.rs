//! Conversions between the parts' raw readings and settings, and units.

use crate::units::{Decibels, Millicelsius, Millivolts, Percent};

/// The charge left for the battery at `voltage`: M5Stack's estimate for
/// the Cardputer (M5Unified's `Power_Class::getBatteryLevel`), 3.30 V
/// empty, 4.10 V full, linear in between.
#[must_use]
pub fn battery_percent(voltage: Millivolts) -> Percent {
    let level = (i32::from(voltage.0) - 3300) * 100 / 800;
    Percent::new(u8::try_from(level.clamp(0, 100)).unwrap_or(100))
}

/// The ES8311's DAC volume register for `volume`, 0% (−95.5 dB) to 100%
/// (0 dB, register 0xBF), evenly in decibels.
pub fn volume_register(volume: Percent) -> u8 {
    let step = u16::from(volume.get()) * 0xBF / 100;
    u8::try_from(step).unwrap_or(0xBF)
}

/// The ES8311 microphone preamp's gain step for `gain`: 0 to 10, 3 dB
/// each, rounded down; more than 30 dB is 30.
pub const fn microphone_gain_step(gain: Decibels) -> u8 {
    if gain.0 > 30 { 10 } else { gain.0 / 3 }
}

/// A BMI270 acceleration reading at ±`full_scale` g (the 16 bits across
/// it: 4096 counts a g at ±8 g), in milli-g.
#[expect(clippy::cast_possible_truncation, reason = "at most 16 g, in milli-g")]
pub const fn milli_g(raw: i32, full_scale: i32) -> i32 {
    (raw as i64 * full_scale as i64 * 1000 / 32768) as i32
}

/// A BMI270 rotation reading at ±`full_scale` °/s (the 16 bits across it:
/// 16.384 counts per °/s at ±2000), in milli-degrees a second.
#[expect(
    clippy::cast_possible_truncation,
    reason = "at most 2000 °/s, in milli-degrees"
)]
pub const fn millidegrees_per_second(raw: i32, full_scale: i32) -> i32 {
    (raw as i64 * full_scale as i64 * 1000 / 32768) as i32
}

/// A BMI270 temperature reading (0 at 23 °C, 512 counts a degree), or
/// `None` for 0x8000: no reading, with both sensors off.
pub const fn millicelsius(raw: i16) -> Option<Millicelsius> {
    if raw == i16::MIN {
        None
    } else {
        Some(Millicelsius(23_000 + raw as i32 * 1000 / 512))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_battery_is_empty_at_3_3_volts_and_full_at_4_1() {
        let percent = |millivolts| battery_percent(Millivolts(millivolts)).get();
        assert_eq!(percent(0), 0);
        assert_eq!(percent(3300), 0);
        assert_eq!(percent(3700), 50);
        assert_eq!(percent(4100), 100);
        // On USB it reads the charging voltage.
        assert_eq!(percent(4200), 100);
        assert_eq!(percent(u16::MAX), 100);
    }

    #[test]
    fn the_volume_runs_from_silent_to_0_db() {
        assert_eq!(volume_register(Percent::ZERO), 0x00);
        assert_eq!(volume_register(Percent::new(50)), 0x5F);
        assert_eq!(volume_register(Percent::FULL), 0xBF);
        assert_eq!(volume_register(Percent::new(255)), 0xBF);
    }

    #[test]
    fn the_microphone_gain_rounds_down_to_3_db_steps() {
        let step = |db| microphone_gain_step(Decibels(db));
        assert_eq!(step(0), 0);
        assert_eq!(step(2), 0);
        assert_eq!(step(24), 8);
        assert_eq!(step(30), 10);
        assert_eq!(step(255), 10);
    }

    #[test]
    fn the_temperature_is_23_degrees_at_zero_and_512_counts_a_degree() {
        assert_eq!(millicelsius(0), Some(Millicelsius(23_000)));
        assert_eq!(millicelsius(512), Some(Millicelsius(24_000)));
        assert_eq!(millicelsius(-256), Some(Millicelsius(22_500)));
        assert_eq!(millicelsius(i16::MAX), Some(Millicelsius(86_998)));
        assert_eq!(millicelsius(i16::MIN), None, "no reading");
    }

    #[test]
    fn motion_readings_scale_to_their_ranges() {
        assert_eq!(milli_g(4096, 8), 1000);
        assert_eq!(milli_g(-4096, 8), -1000);
        assert_eq!(milli_g(i32::from(i16::MIN), 8), -8000);
        assert_eq!(milli_g(16_384, 2), 1000);
        assert_eq!(milli_g(i32::from(i16::MIN), 16), -16_000);
        assert_eq!(millidegrees_per_second(16_384, 2000), 1_000_000);
        assert_eq!(
            millidegrees_per_second(i32::from(i16::MAX), 2000),
            1_999_938
        );
        assert_eq!(
            millidegrees_per_second(i32::from(i16::MIN), 2000),
            -2_000_000
        );
        assert_eq!(millidegrees_per_second(262, 125), 999);
        assert_eq!(millidegrees_per_second(i32::from(i16::MIN), 125), -125_000);
    }
}
