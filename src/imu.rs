//! The motion sensor: a BMI270 on the I2C bus.

use core::{array, fmt};

use bmi2::config::BMI270_CONFIG_FILE;
use embassy_time::{Duration, Timer};
use embedded_hal_async::i2c::I2c as _;

use crate::{
    Acceleration, Error, I2cBus, I2cDevice, Millicelsius, Motion, Rotation,
    bmi270::{
        self, ACCELEROMETER_ON, ADVANCED_POWER_SAVE, AccelerometerRange, GYROSCOPE_ON,
        GyroscopeRange, INITIALISED, ImuMode, ImuRate, SOFT_RESET, UPLOAD_CHUNK, register,
    },
    convert::{milli_g, millicelsius, millidegrees_per_second},
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
/// measurement: a third of a second at 100 Hz.
const CALIBRATION_SAMPLES: i32 = 32;
/// How long a new range takes to reach the readings. Setting one restarts
/// the sensor's filter, and the last reading, in the old range, stays for up
/// to 60 ms (measured on the ADV).
const RANGE_CHANGE: Duration = Duration::from_millis(80);

/// The BMI270 motion sensor: its accelerometer, and its gyroscope once
/// [turned on](Self::set_gyroscope).
///
/// Both sample at 100 Hz, in its [power mode](Self::set_mode), at ±8 g
/// and ±2000 °/s unless [set](Self::set_rate)
/// [otherwise](Self::set_accelerometer_range)
/// ([and](Self::set_gyroscope_range)).
pub struct Imu {
    device: I2cDevice,
    /// Where it answered: 0x68 or 0x69.
    address: u8,
    mode: ImuMode,
    rate: ImuRate,
    accelerometer_range: AccelerometerRange,
    gyroscope_range: GyroscopeRange,
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
        let raw = self.axes(register::ACC_DATA).await?;
        Ok(self.acceleration_from(raw))
    }

    /// Sets the accelerometer's range: ±2 g reads tilt four times as finely
    /// as the ±8 g [`imu`] starts at, and clips anything past 2 g (a
    /// shake). Readings are in milli-g whatever the range. It takes 80 ms,
    /// so the next reading is in the new range.
    ///
    /// # Errors
    ///
    /// If the sensor doesn't answer.
    pub async fn set_accelerometer_range(
        &mut self,
        range: AccelerometerRange,
    ) -> Result<(), Error> {
        self.write(register::ACC_RANGE, range.register()).await?;
        Timer::after(RANGE_CHANGE).await;
        self.accelerometer_range = range;
        Ok(())
    }

    /// The accelerometer's range.
    #[must_use]
    pub const fn accelerometer_range(&self) -> AccelerometerRange {
        self.accelerometer_range
    }

    /// Sets the gyroscope's range: a narrower one reads slow turns more
    /// finely, and clips fast ones. Readings are in milli-degrees a second
    /// whatever the range, and a [calibration](Self::calibrate_gyroscope)
    /// carries over. It applies whether the gyroscope is on or not, and
    /// takes 80 ms, so the next reading is in the new range.
    ///
    /// # Errors
    ///
    /// If the sensor doesn't answer.
    pub async fn set_gyroscope_range(&mut self, range: GyroscopeRange) -> Result<(), Error> {
        self.write(register::GYR_RANGE, range.register()).await?;
        Timer::after(RANGE_CHANGE).await;
        // The offset is in counts, which the range scales.
        let (from, to) = (self.gyroscope_range.full_scale(), range.full_scale());
        self.gyroscope_offset = self.gyroscope_offset.map(|offset| offset * from / to);
        self.gyroscope_range = range;
        Ok(())
    }

    /// The gyroscope's range.
    #[must_use]
    pub const fn gyroscope_range(&self) -> GyroscopeRange {
        self.gyroscope_range
    }

    /// The acceleration and the rotation rate now, in one read, so both are
    /// of the same moment: for combining them (tilt, gestures), and half
    /// the bus traffic of reading them apart. The rotation is as
    /// [`rotation`](Self::rotation)'s, and zero while the gyroscope is off.
    ///
    /// # Errors
    ///
    /// If the sensor can't be read.
    pub async fn motion(&mut self) -> Result<Motion, Error> {
        // The gyroscope's registers follow the accelerometer's.
        let mut data = [0; 12];
        self.device
            .write_read(self.address, &[register::ACC_DATA], &mut data)
            .await
            .map_err(|_| Error::Imu)?;
        let [a0, a1, a2, a3, a4, a5, g0, g1, g2, g3, g4, g5] = data;
        Ok(Motion {
            acceleration: self.acceleration_from(bmi270::axes([a0, a1, a2, a3, a4, a5])),
            rotation: self.rotation_from(bmi270::axes([g0, g1, g2, g3, g4, g5])),
        })
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
    /// [`Error::Unsupported`] for [`ImuMode::LowPower`] at a
    /// [rate](Self::set_rate) above 200 Hz, which it can't sample at;
    /// [`Error::Imu`] if the sensor doesn't answer, and the mode is then
    /// partly set.
    pub async fn set_mode(&mut self, mode: ImuMode) -> Result<(), Error> {
        self.configure(mode, self.rate).await
    }

    /// The power mode the sensors are in.
    #[must_use]
    pub const fn mode(&self) -> ImuMode {
        self.mode
    }

    /// Sets how often both sensors measure: faster catches quicker
    /// movements (taps, vibration), and slower draws less in
    /// [`ImuMode::LowPower`]. [`imu`] starts at 100 Hz.
    ///
    /// As after a change of [mode](Self::set_mode), the readings stay as
    /// they were for a few tens of milliseconds while the sensor's filter
    /// starts again.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] above 200 Hz in [`ImuMode::LowPower`], which
    /// can't sample that fast; [`Error::Imu`] if the sensor doesn't answer,
    /// and the rate is then partly set.
    pub async fn set_rate(&mut self, rate: ImuRate) -> Result<(), Error> {
        self.configure(self.mode, rate).await
    }

    /// How often the sensors measure.
    #[must_use]
    pub const fn rate(&self) -> ImuRate {
        self.rate
    }

    /// Writes the sensors' configuration for `mode` at `rate`.
    async fn configure(&mut self, mode: ImuMode, rate: ImuRate) -> Result<(), Error> {
        if !mode.supports(rate) {
            return Err(Error::Unsupported);
        }
        self.write(register::ACC_CONF, mode.acc_conf(rate)).await?;
        self.write(register::GYR_CONF, mode.gyr_conf(rate)).await?;
        (self.mode, self.rate) = (mode, rate);
        Ok(())
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
            self.write(register::GYR_RANGE, self.gyroscope_range.register())
                .await?;
            self.write(register::GYR_CONF, self.mode.gyr_conf(self.rate))
                .await?;
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
        Ok(self.rotation_from(raw))
    }

    /// The sensor's temperature: the board's, near enough, a little above
    /// the room's while the Cardputer runs. It's measured every 1.28 s.
    ///
    /// # Errors
    ///
    /// If the sensor can't be read, or has no reading.
    pub async fn temperature(&mut self) -> Result<Millicelsius, Error> {
        let mut data = [0; 2];
        self.device
            .write_read(self.address, &[register::TEMPERATURE], &mut data)
            .await
            .map_err(|_| Error::Imu)?;
        millicelsius(i16::from_le_bytes(data)).ok_or(Error::Imu)
    }

    /// Measures the gyroscope's offset (what it reads at rest), and takes it
    /// off [`rotation`](Self::rotation)'s readings from then on. Turns the
    /// gyroscope on if it isn't.
    ///
    /// The Cardputer must lie still meanwhile, about 0.4 s at 100 Hz (32
    /// readings, so longer at slower [rates](Self::set_rate)): any turning is
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
        let period = Duration::from_micros(
            u64::try_from(self.rate.period().as_micros()).unwrap_or(u64::MAX),
        );
        for _ in 0..CALIBRATION_SAMPLES {
            Timer::after(period).await;
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
        self.write(register::ACC_RANGE, self.accelerometer_range.register())
            .await?;
        self.write(register::ACC_CONF, self.mode.acc_conf(self.rate))
            .await?;
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

    /// An accelerometer reading in the sensor's counts, in milli-g.
    fn acceleration_from(&self, raw: [i32; 3]) -> Acceleration {
        let full_scale = self.accelerometer_range.full_scale();
        let [x, y, z] = raw.map(|count| milli_g(count, full_scale));
        Acceleration { x, y, z }
    }

    /// A gyroscope reading in the sensor's counts as a rotation rate, its
    /// offset taken off.
    fn rotation_from(&self, raw: [i32; 3]) -> Rotation {
        let full_scale = self.gyroscope_range.full_scale();
        let [x, y, z] = array::from_fn(|axis| {
            millidegrees_per_second(raw[axis] - self.gyroscope_offset[axis], full_scale)
        });
        Rotation { x, y, z }
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
/// uploads its configuration and turns the accelerometer on.
///
/// It returns after about 230 ms, most of it the 8 kB configuration going
/// over the bus at 400 kHz; other tasks share the bus meanwhile.
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
            rate: ImuRate::default(),
            accelerometer_range: AccelerometerRange::default(),
            gyroscope_range: GyroscopeRange::default(),
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
            .field("rate", &self.rate)
            .field("accelerometer_range", &self.accelerometer_range)
            .field("gyroscope_range", &self.gyroscope_range)
            .field("gyroscope_offset", &self.gyroscope_offset)
            .finish_non_exhaustive()
    }
}
