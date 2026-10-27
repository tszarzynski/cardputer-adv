//! Conversions between the parts' raw readings and settings, and units.

/// The charge left in percent for the battery at `millivolts`: M5Stack's
/// estimate for the Cardputer (M5Unified's `Power_Class::getBatteryLevel`),
/// 3.30 V empty, 4.10 V full, linear in between.
#[must_use]
pub fn battery_percent(millivolts: u16) -> u8 {
    let level = (i32::from(millivolts) - 3300) * 100 / 800;
    u8::try_from(level.clamp(0, 100)).unwrap_or(100)
}

/// The ES8311's DAC volume register for `percent`, 0 (−95.5 dB) to 100
/// (0 dB, register 0xBF), evenly in decibels; more than 100 is 100.
pub fn volume_register(percent: u8) -> u8 {
    let step = u16::from(percent.min(100)) * 0xBF / 100;
    u8::try_from(step).unwrap_or(0xBF)
}

/// The ES8311 microphone preamp's gain step for `db`: 0 to 10, 3 dB each,
/// rounded down; more than 30 dB is 30.
pub const fn microphone_gain_step(db: u8) -> u8 {
    if db > 30 { 10 } else { db / 3 }
}

/// A BMI270 acceleration reading at ±8 g (4096 counts a g), in milli-g.
pub const fn milli_g(raw: i32) -> i32 {
    raw * 1000 / 4096
}

/// A BMI270 rotation reading at ±2000 °/s (16.384 counts per °/s), in
/// milli-degrees a second.
pub const fn millidegrees_per_second(raw: i32) -> i32 {
    raw * 15_625 / 256
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_battery_is_empty_at_3_3_volts_and_full_at_4_1() {
        assert_eq!(battery_percent(0), 0);
        assert_eq!(battery_percent(3300), 0);
        assert_eq!(battery_percent(3700), 50);
        assert_eq!(battery_percent(4100), 100);
        // On USB it reads the charging voltage.
        assert_eq!(battery_percent(4200), 100);
        assert_eq!(battery_percent(u16::MAX), 100);
    }

    #[test]
    fn the_volume_runs_from_silent_to_0_db() {
        assert_eq!(volume_register(0), 0x00);
        assert_eq!(volume_register(50), 0x5F);
        assert_eq!(volume_register(100), 0xBF);
        assert_eq!(volume_register(255), 0xBF);
    }

    #[test]
    fn the_microphone_gain_rounds_down_to_3_db_steps() {
        assert_eq!(microphone_gain_step(0), 0);
        assert_eq!(microphone_gain_step(2), 0);
        assert_eq!(microphone_gain_step(24), 8);
        assert_eq!(microphone_gain_step(30), 10);
        assert_eq!(microphone_gain_step(255), 10);
    }

    #[test]
    fn motion_readings_scale_to_their_ranges() {
        assert_eq!(milli_g(4096), 1000);
        assert_eq!(milli_g(-4096), -1000);
        assert_eq!(milli_g(i32::from(i16::MIN)), -8000);
        assert_eq!(millidegrees_per_second(16_384), 1_000_000);
        assert_eq!(millidegrees_per_second(i32::from(i16::MAX)), 1_999_938);
        assert_eq!(millidegrees_per_second(i32::from(i16::MIN)), -2_000_000);
    }
}
