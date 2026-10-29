//! The I2C bus the keyboard, the codec and the motion sensor share.

use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice as SharedDevice;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use esp_hal::{
    Async,
    i2c::master::{Config as I2cConfig, I2c},
    peripherals::{GPIO8, GPIO9, I2C0},
    time::Rate,
};
use static_cell::StaticCell;

use crate::Error;

/// The I2C bus: the keyboard, the ES8311 codec and the BMI270 share it.
///
/// Its transfers are async, and an async mutex takes turns between its
/// devices: a task waiting for the bus, or for a transfer, lets the others
/// run, and interrupts aren't held off. The mutex is a critical-section
/// one, so the devices can be used from any executor or core.
pub type I2cBus = Mutex<CriticalSectionRawMutex, I2c<'static, Async>>;

/// One device's handle on the shared [`I2cBus`].
pub type I2cDevice = SharedDevice<'static, CriticalSectionRawMutex, I2c<'static, Async>>;

/// The peripherals the I2C bus uses.
#[expect(missing_docs, reason = "the fields are the pins they're named after")]
#[derive(Debug)]
pub struct I2cPins {
    pub i2c: I2C0<'static>,
    pub sda: GPIO8<'static>,
    pub scl: GPIO9<'static>,
}

/// Sets up the shared I2C bus (400 kHz).
///
/// # Errors
///
/// If the bus can't be configured.
///
/// # Panics
///
/// If called twice (there is one bus).
pub fn i2c(pins: I2cPins) -> Result<&'static I2cBus, Error> {
    static BUS: StaticCell<I2cBus> = StaticCell::new();
    let i2c = I2c::new(
        pins.i2c,
        I2cConfig::default().with_frequency(Rate::from_khz(400)),
    )
    .map_err(|_| Error::I2c)?
    .with_sda(pins.sda)
    .with_scl(pins.scl)
    .into_async();
    Ok(BUS.init(Mutex::new(i2c)))
}
