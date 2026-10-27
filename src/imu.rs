//! The motion sensor: a BMI270 on the I2C bus.

use core::{array, fmt};

use bmi2::{
    Bmi2, I2cAddr, config,
    interface::I2cInterface,
    types::{
        AccBwp, AccConf, AccRange, Burst, GyrBwp, GyrConf, GyrRange, GyrRangeVal, Odr, OisRange,
        PerfMode, PwrCtrl,
    },
};
use embassy_time::{Duration, Timer};
use embedded_hal_bus::i2c::CriticalSectionDevice;
use esp_hal::delay::Delay;

use crate::{Error, I2cBus, I2cDevice};

/// The largest I2C write while uploading the BMI270's configuration.
const IMU_BURST: u16 = 128;

/// The BMI270 driver on the shared bus.
type Bmi270 = Bmi2<I2cInterface<I2cDevice>, Delay, { IMU_BURST as usize }>;

/// How long the gyroscope takes to give its first readings once on.
const GYROSCOPE_START: Duration = Duration::from_millis(50);

/// How many readings [`Imu::calibrate_gyroscope`] averages, one per
/// measurement (100 Hz): a third of a second.
const CALIBRATION_SAMPLES: i32 = 32;
const SAMPLE_PERIOD: Duration = Duration::from_millis(10);

/// The BMI270: its accelerometer, ±8 g at 100 Hz, and its gyroscope,
/// ±2000 °/s at 100 Hz once [turned on](Self::set_gyroscope).
pub struct Imu {
    bmi: Bmi270,
    /// The gyroscope's raw reading at rest, taken off each reading.
    gyroscope_offset: [i32; 3],
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
    /// the sensor's axes.
    ///
    /// Uncalibrated, it can read several degrees a second while still (from
    /// 0.5 to 9 °/s in different runs on the ADV tested); after
    /// [`calibrate_gyroscope`](Self::calibrate_gyroscope), within a few
    /// hundred milli-degrees of zero.
    ///
    /// # Errors
    ///
    /// If the sensor can't be read.
    pub fn rotation(&mut self) -> Result<[i32; 3], Error> {
        let raw = self.gyroscope_raw()?;
        // ±2000 °/s over the i16 range: 16.384 counts per °/s.
        Ok(array::from_fn(|axis| {
            (raw[axis] - self.gyroscope_offset[axis]) * 15_625 / 256
        }))
    }

    /// Measures the gyroscope's offset (what it reads at rest), and takes it
    /// off [`rotation`](Self::rotation)'s readings from then on. Turns the
    /// gyroscope on if it isn't.
    ///
    /// The Cardputer must lie still meanwhile, about 0.4 s: any turning is
    /// taken for offset. The offset drifts with temperature, so calibrate
    /// again if it matters.
    ///
    /// # Errors
    ///
    /// If the sensor can't be read.
    pub async fn calibrate_gyroscope(&mut self) -> Result<(), Error> {
        self.set_gyroscope(true)?;
        Timer::after(GYROSCOPE_START).await;
        let mut sum = [0; 3];
        for _ in 0..CALIBRATION_SAMPLES {
            Timer::after(SAMPLE_PERIOD).await;
            let raw = self.gyroscope_raw()?;
            for (total, value) in sum.iter_mut().zip(raw) {
                *total += value;
            }
        }
        self.gyroscope_offset = sum.map(|total| total / CALIBRATION_SAMPLES);
        Ok(())
    }

    /// The gyroscope's reading, in its counts.
    fn gyroscope_raw(&mut self) -> Result<[i32; 3], Error> {
        let data = self.bmi.get_gyr_data().map_err(|_| Error::Imu)?;
        Ok([data.x, data.y, data.z].map(i32::from))
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
    let mut imu = Imu {
        bmi,
        gyroscope_offset: [0; 3],
    };
    imu.set_power(false)?;
    Ok(imu)
}

impl fmt::Debug for Imu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Imu")
            .field("gyroscope_offset", &self.gyroscope_offset)
            .finish_non_exhaustive()
    }
}
