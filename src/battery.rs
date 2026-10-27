//! The battery voltage, and the charge it suggests.

use core::fmt;

use esp_hal::{
    Blocking,
    analog::adc::{Adc, AdcCalCurve, AdcConfig, AdcPin, Attenuation},
    peripherals::{ADC1, GPIO10},
};

/// The peripherals the battery measurement uses.
#[derive(Debug)]
pub struct BatteryPins {
    /// The ADC the battery is read with.
    pub adc: ADC1<'static>,
    /// The battery, through the board's 2:1 divider.
    pub pin: GPIO10<'static>,
}

/// How many readings [`Battery::millivolts`] averages.
const BATTERY_SAMPLES: u32 = 16;

/// The battery voltage, measured as M5Stack's M5Unified does for the
/// Cardputer and Cardputer ADV: GPIO10 on ADC1, through a 2:1 divider.
pub struct Battery {
    adc: Adc<'static, ADC1<'static>, Blocking>,
    pin: AdcPin<GPIO10<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
}

impl Battery {
    /// The battery voltage in millivolts, averaged over a few readings.
    ///
    /// The board can't tell whether it's charging: on USB with the power
    /// switch on, this is the charging voltage, close to full.
    pub fn millivolts(&mut self) -> u16 {
        let sum: u32 = (0..BATTERY_SAMPLES)
            .map(|_| u32::from(self.adc.read_blocking(&mut self.pin)))
            .sum();
        u16::try_from(sum / BATTERY_SAMPLES * 2).unwrap_or(u16::MAX)
    }

    /// The charge left, in percent, estimated from [`millivolts`](Self::millivolts)
    /// ([`battery_percent`]). Reads high on USB, where it can't be told from
    /// charging.
    pub fn percent(&mut self) -> u8 {
        battery_percent(self.millivolts())
    }
}

/// The charge left in percent for the battery at `millivolts`: M5Stack's
/// estimate for the Cardputer (M5Unified's `Power_Class::getBatteryLevel`),
/// 3.30 V empty, 4.10 V full, linear in between.
#[must_use]
pub fn battery_percent(millivolts: u16) -> u8 {
    let level = (i32::from(millivolts) - 3300) * 100 / 800;
    u8::try_from(level.clamp(0, 100)).unwrap_or(100)
}

/// Sets up the battery measurement: ADC1 with the chip's calibration curve,
/// at 11 dB attenuation (up to about 3.1 V at the pin, 6.2 V at the battery).
#[must_use]
pub fn battery(pins: BatteryPins) -> Battery {
    let mut config = AdcConfig::new();
    let pin =
        config.enable_pin_with_cal::<_, AdcCalCurve<ADC1<'static>>>(pins.pin, Attenuation::_11dB);
    Battery {
        adc: Adc::new(pins.adc, config),
        pin,
    }
}

impl fmt::Debug for Battery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Battery").finish_non_exhaustive()
    }
}
