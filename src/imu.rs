//! The motion sensor: a BMI270 on the I2C bus.

use core::fmt;

use bmi2::{
    Bmi2, I2cAddr, config,
    interface::I2cInterface,
    types::{
        AccBwp, AccConf, AccRange, Burst, GyrBwp, GyrConf, GyrRange, GyrRangeVal, Odr, OisRange,
        PerfMode, PwrCtrl,
    },
};
use embedded_hal_bus::i2c::CriticalSectionDevice;
use esp_hal::delay::Delay;

use crate::{Error, I2cBus, I2cDevice};

/// The largest I2C write while uploading the BMI270's configuration.
const IMU_BURST: u16 = 128;

/// The BMI270 driver on the shared bus.
type Bmi270 = Bmi2<I2cInterface<I2cDevice>, Delay, { IMU_BURST as usize }>;

/// The BMI270: its accelerometer, ±8 g at 100 Hz, and its gyroscope,
/// ±2000 °/s at 100 Hz once [turned on](Self::set_gyroscope).
pub struct Imu {
    bmi: Bmi270,
}

impl Imu {
    /// The acceleration now, in milli-g, as (x, y, z) in the sensor's axes.
    /// At rest it is about 1000 mg in total: gravity.
    ///
    /// # Errors
    ///
    /// If the sensor can't be read.
    pub fn acceleration(&mut self) -> Result<[i32; 3], Error> {
        let data = self.bmi.get_acc_data().map_err(|_| Error::Imu)?;
        // ±8 g over the i16 range: 4096 counts per g.
        let mg = |raw: i16| i32::from(raw) * 1000 / 4096;
        Ok([mg(data.x), mg(data.y), mg(data.z)])
    }

    /// Turns the gyroscope on or off. It draws about 0.9 mA, so [`imu`]
    /// leaves it off; once on, its first readings come after about 45 ms.
    ///
    /// # Errors
    ///
    /// If the sensor doesn't answer.
    pub fn set_gyroscope(&mut self, on: bool) -> Result<(), Error> {
        if on {
            self.bmi
                .set_gyr_range(GyrRange {
                    range: GyrRangeVal::Range2000,
                    ois_range: OisRange::Range2000,
                })
                .map_err(|_| Error::Imu)?;
            self.bmi
                .set_gyr_conf(GyrConf {
                    odr: Odr::Odr100,
                    bwp: GyrBwp::Norm,
                    noise_perf: PerfMode::Perf,
                    filter_perf: PerfMode::Perf,
                })
                .map_err(|_| Error::Imu)?;
        }
        self.set_power(on)
    }

    /// The rotation rate now, in milli-degrees a second, as (x, y, z) about
    /// the sensor's axes. Zero (or nearly: a few hundred either way) while
    /// still, and while the [gyroscope](Self::set_gyroscope) is off.
    ///
    /// # Errors
    ///
    /// If the sensor can't be read.
    pub fn rotation(&mut self) -> Result<[i32; 3], Error> {
        let data = self.bmi.get_gyr_data().map_err(|_| Error::Imu)?;
        // ±2000 °/s over the i16 range: 16.384 counts per °/s.
        let mdps = |raw: i16| i32::from(raw) * 15_625 / 256;
        Ok([mdps(data.x), mdps(data.y), mdps(data.z)])
    }

    /// Powers the accelerometer, and the gyroscope if `gyroscope`.
    fn set_power(&mut self, gyroscope: bool) -> Result<(), Error> {
        self.bmi
            .set_pwr_ctrl(PwrCtrl {
                aux_en: false,
                gyr_en: gyroscope,
                acc_en: true,
                temp_en: false,
            })
            .map_err(|_| Error::Imu)
    }
}

/// Starts the BMI270 on the shared bus: finds it at either of its addresses,
/// uploads its configuration (8 kB, about 50 ms) and turns the accelerometer
/// on.
///
/// # Errors
///
/// If no BMI270 answers, or it doesn't start.
pub fn imu(bus: &'static I2cBus) -> Result<Imu, Error> {
    let mut bmi = [I2cAddr::Default, I2cAddr::Alternative]
        .into_iter()
        .find_map(|address| {
            let mut bmi = Bmi270::new_i2c(
                CriticalSectionDevice::new(bus),
                Delay::new(),
                address,
                Burst::new(IMU_BURST),
            );
            bmi.get_chip_id().is_ok_and(|id| id == 0x24).then_some(bmi)
        })
        .ok_or(Error::Imu)?;
    bmi.init(&config::BMI270_CONFIG_FILE)
        .map_err(|_| Error::Imu)?;
    bmi.set_acc_range(AccRange::Range8g)
        .map_err(|_| Error::Imu)?;
    bmi.set_acc_conf(AccConf {
        odr: Odr::Odr100,
        bwp: AccBwp::NormAvg4,
        filter_perf: PerfMode::Perf,
    })
    .map_err(|_| Error::Imu)?;
    let mut imu = Imu { bmi };
    imu.set_power(false)?;
    Ok(imu)
}

impl fmt::Debug for Imu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Imu").finish_non_exhaustive()
    }
}
