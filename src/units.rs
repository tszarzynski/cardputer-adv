//! The units the parts' readings and settings come in, so a level can't be
//! passed where a voltage is meant, or the axes of a reading mixed up.

use core::fmt;

/// A share of the whole, 0 to 100: a brightness, a volume or a charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Percent(u8);

impl Percent {
    /// None of it: off, silent or empty.
    pub const ZERO: Self = Self(0);
    /// All of it: full brightness, full volume or a full battery.
    pub const FULL: Self = Self(100);

    /// `value` percent; more than 100 is 100.
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(if value > 100 { 100 } else { value })
    }

    /// The value, 0 to 100.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<Percent> for u8 {
    fn from(percent: Percent) -> Self {
        percent.0
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

/// A voltage, in millivolts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Millivolts(pub u16);

impl fmt::Display for Millivolts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} mV", self.0)
    }
}

/// A gain, in decibels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Decibels(pub u8);

impl fmt::Display for Decibels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} dB", self.0)
    }
}

/// A frequency, in hertz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Hertz(pub u32);

impl fmt::Display for Hertz {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Hz", self.0)
    }
}

/// A temperature, in thousandths of a degree Celsius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Millicelsius(pub i32);

/// To a tenth of a degree, rounded toward zero: `23.5 °C`.
impl fmt::Display for Millicelsius {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let tenths = self.0.unsigned_abs() / 100;
        write!(f, "{sign}{}.{} °C", tenths / 10, tenths % 10)
    }
}

/// An acceleration, in milli-g, along the motion sensor's axes. At rest
/// it's about 1000 mg in all: gravity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Acceleration {
    /// Along the sensor's x axis.
    pub x: i32,
    /// Along the sensor's y axis.
    pub y: i32,
    /// Along the sensor's z axis.
    pub z: i32,
}

/// A rotation rate, in milli-degrees a second, about the motion sensor's
/// axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Rotation {
    /// About the sensor's x axis.
    pub x: i32,
    /// About the sensor's y axis.
    pub y: i32,
    /// About the sensor's z axis.
    pub z: i32,
}

/// The motion sensor's two readings, taken at the same moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Motion {
    /// The acceleration, in milli-g.
    pub acceleration: Acceleration,
    /// The rotation rate, in milli-degrees a second.
    pub rotation: Rotation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_percentage_stops_at_100() {
        assert_eq!(Percent::new(0), Percent::ZERO);
        assert_eq!(Percent::new(42).get(), 42);
        assert_eq!(Percent::new(100), Percent::FULL);
        assert_eq!(Percent::new(255), Percent::FULL);
        assert_eq!(u8::from(Percent::new(7)), 7);
    }

    #[test]
    fn units_show_with_their_symbols() {
        assert_eq!(Percent::new(15).to_string(), "15%");
        assert_eq!(Millivolts(3880).to_string(), "3880 mV");
        assert_eq!(Decibels(24).to_string(), "24 dB");
        assert_eq!(Hertz(440).to_string(), "440 Hz");
        assert_eq!(Millicelsius(23_456).to_string(), "23.4 °C");
        assert_eq!(Millicelsius(-1_250).to_string(), "-1.2 °C");
        assert_eq!(Millicelsius(-50).to_string(), "-0.0 °C");
    }
}
