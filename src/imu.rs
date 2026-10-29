//! The motion sensor: a BMI270 on the I2C bus.

use core::{array, fmt};

use bmi2::config::BMI270_CONFIG_FILE;
use embassy_time::{Duration, Timer};
use embedded_hal_async::i2c::I2c as _;

use crate::{
    Acceleration, Error, I2cBus, I2cDevice, Rotation,
    bmi270::{
        self, ACC_RANGE_8G, ACCELEROMETER_ON, ADVANCED_POWER_SAVE, GYR_RANGE_2000, GYROSCOPE_ON,
        INITIALISED, ImuMode, SOFT_RESET, UPLOAD_CHUNK, register,
    },
    convert::{milli_g, millidegrees_per_second},
};

/// How long it takes to come back from a soft reset.
const RESET_TIME: Duration = Duration::from_millis(2);
/// The wait after each register write: with advanced power save on, the
/// BMI270 takes one write per 450 µs.
const WRITE_GAP: Duration = Duration::from_micros(450);
/// How long it takes to start on the configuration (20 ms at most).
const INITIALISATION_TIME: Duration = Duration::from_millis(20);

/// How long the gyroscope takes to give its first readings once on.
const GYROSCOPE_START: Duration = Duration::from_millis(50);

/// How many readings [`Imu::calibrate_gyroscope`] averages, one per
/// measurement (100 Hz): a third of a second.
const CALIBRATION_SAMPLES: i32 = 32;
const SAMPLE_PERIOD: Duration = Duration::from_millis(10);

/// The BMI270: its accelerometer, ±8 g at 100 Hz, and its gyroscope,
/// ±2000 °/s at 100 Hz once [turned on](Self::set_gyroscope), both in its
/// [power mode](Self::set_mode).
pub struct Imu {
    device: I2cDevice,
    /// Where it answered: 0x68 or 0x69.
    address: u8,
    mode: ImuMode,
    /// The gyroscope's raw reading at rest, taken off each reading.
    gyroscope_offset: [i32; 3],
}

impl Imu {
    /// The acceleration now. At rest it is about 1000 mg in total: gravity.
    ///
    /// # Errors
    ///
    /// If the sensor can't be read.
    pub async fn acceleration(&mut self) -> Result<Acceleration, Error> {
        let [x, y, z] = self.axes(register::ACC_DATA).await?.map(milli_g);
        Ok(Acceleration { x, y, z })
    }

    /// Sets the power mode: how much current the sensors draw, against how
    /// noisy their readings are. [`imu`] starts in
    /// [`ImuMode::Performance`]; [`ImuMode::LowPower`] suits tilt and shakes
    /// on battery.
    ///
    /// The gyroscope's offset can differ between modes: calibrate it
    /// ([`calibrate_gyroscope`](Self::calibrate_gyroscope)) after changing.
    ///
    /// # Errors
    ///
    /// If the sensor doesn't answer; the mode is then partly set.
    pub async fn set_mode(&mut self, mode: ImuMode) -> Result<(), Error> {
        self.write(register::ACC_CONF, mode.acc_conf()).await?;
        self.write(register::GYR_CONF, mode.gyr_conf()).await?;
        self.mode = mode;
        Ok(())
    }

    /// The power mode the sensors are in.
    #[must_use]
    pub const fn mode(&self) -> ImuMode {
        self.mode
    }

    /// Turns the gyroscope on or off, in the [power mode](Self::set_mode).
    /// [`imu`] leaves it off, as it draws far more than the accelerometer:
    /// 685 µA with it, against 210 µA alone, in [`ImuMode::Normal`]. Once
    /// on, its first readings come after about 45 ms.
    ///
    /// # Errors
    ///
    /// If the sensor doesn't answer.
    pub async fn set_gyroscope(&mut self, on: bool) -> Result<(), Error> {
        if on {
            self.write(register::GYR_RANGE, GYR_RANGE_2000).await?;
            self.write(register::GYR_CONF, self.mode.gyr_conf()).await?;
        }
        self.set_power(on).await
    }

    /// The rotation rate now.
    ///
    /// Uncalibrated, it can read several degrees a second while still (from
    /// 0.5 to 9 °/s in different runs on the ADV tested); after
    /// [`calibrate_gyroscope`](Self::calibrate_gyroscope), within a few
    /// hundred milli-degrees of zero.
    ///
    /// # Errors
    ///
    /// If the sensor can't be read.
    pub async fn rotation(&mut self) -> Result<Rotation, Error> {
        let raw = self.axes(register::GYR_DATA).await?;
        let [x, y, z] =
            array::from_fn(|axis| millidegrees_per_second(raw[axis] - self.gyroscope_offset[axis]));
        Ok(Rotation { x, y, z })
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
        self.set_gyroscope(true).await?;
        Timer::after(GYROSCOPE_START).await;
        let mut sum = [0; 3];
        for _ in 0..CALIBRATION_SAMPLES {
            Timer::after(SAMPLE_PERIOD).await;
            let raw = self.axes(register::GYR_DATA).await?;
            for (total, value) in sum.iter_mut().zip(raw) {
                *total += value;
            }
        }
        self.gyroscope_offset = sum.map(|total| total / CALIBRATION_SAMPLES);
        Ok(())
    }

    /// Powers the accelerometer, and the gyroscope if `gyroscope`.
    async fn set_power(&mut self, gyroscope: bool) -> Result<(), Error> {
        let gyroscope = if gyroscope { GYROSCOPE_ON } else { 0 };
        self.write(register::PWR_CTRL, ACCELEROMETER_ON | gyroscope)
            .await
    }

    /// Resets it, uploads its configuration, and turns the accelerometer on
    /// at ±8 g and 100 Hz.
    async fn set_up(&mut self) -> Result<(), Error> {
        self.write(register::CMD, SOFT_RESET).await?;
        Timer::after(RESET_TIME).await;
        self.set_power_save(false).await?;
        let control = self.read(register::INIT_CTRL).await?;
        self.write(register::INIT_CTRL, control & !1).await?;
        let mut buffer = [0; UPLOAD_CHUNK + 1];
        buffer[0] = register::INIT_DATA;
        for ([address_low, address_high], chunk) in bmi270::upload_chunks(&BMI270_CONFIG_FILE) {
            self.write_all(&[register::INIT_ADDR_0, address_low, address_high])
                .await?;
            buffer[1..=chunk.len()].copy_from_slice(chunk);
            self.write_all(&buffer[..=chunk.len()]).await?;
        }
        self.write(register::INIT_CTRL, 1).await?;
        self.set_power_save(true).await?;
        Timer::after(INITIALISATION_TIME).await;
        if self.read(register::INTERNAL_STATUS).await? & INITIALISED == 0 {
            return Err(Error::Imu);
        }
        self.write(register::ACC_RANGE, ACC_RANGE_8G).await?;
        self.write(register::ACC_CONF, self.mode.acc_conf()).await?;
        self.set_power(false).await
    }

    /// Turns advanced power save on or off.
    async fn set_power_save(&mut self, on: bool) -> Result<(), Error> {
        let config = self.read(register::PWR_CONF).await?;
        let config = if on {
            config | ADVANCED_POWER_SAVE
        } else {
            config & !ADVANCED_POWER_SAVE
        };
        self.write(register::PWR_CONF, config).await
    }

    /// A reading's three axes, in the sensor's counts, from `register` on.
    async fn axes(&mut self, register: u8) -> Result<[i32; 3], Error> {
        let mut data = [0; 6];
        self.device
            .write_read(self.address, &[register], &mut data)
            .await
            .map_err(|_| Error::Imu)?;
        Ok(bmi270::axes(data))
    }

    async fn read(&mut self, register: u8) -> Result<u8, Error> {
        let mut value = [0];
        self.device
            .write_read(self.address, &[register], &mut value)
            .await
            .map_err(|_| Error::Imu)?;
        Ok(value[0])
    }

    /// Writes a register, then waits [`WRITE_GAP`] for the next write.
    async fn write(&mut self, register: u8, value: u8) -> Result<(), Error> {
        self.write_all(&[register, value]).await?;
        Timer::after(WRITE_GAP).await;
        Ok(())
    }

    /// Writes `bytes`: a register, and what goes in it and those after. For
    /// the configuration's upload, which runs with advanced power save off.
    async fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.device
            .write(self.address, bytes)
            .await
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
pub async fn imu(bus: &'static I2cBus) -> Result<Imu, Error> {
    for address in bmi270::ADDRESSES {
        let mut imu = Imu {
            device: I2cDevice::new(bus),
            address,
            mode: ImuMode::default(),
            gyroscope_offset: [0; 3],
        };
        if imu
            .read(register::CHIP_ID)
            .await
            .is_ok_and(|id| id == bmi270::CHIP_ID)
        {
            imu.set_up().await?;
            return Ok(imu);
        }
    }
    Err(Error::Imu)
}

impl fmt::Debug for Imu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Imu")
            .field("address", &self.address)
            .field("mode", &self.mode)
            .field("gyroscope_offset", &self.gyroscope_offset)
            .finish_non_exhaustive()
    }
}
