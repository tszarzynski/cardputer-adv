//! The I2C bus the keyboard, the codec and the motion sensor share.

use core::cell::RefCell;

use critical_section::Mutex;
use embedded_hal_bus::i2c::CriticalSectionDevice;
use esp_hal::{
    Blocking,
    i2c::master::{Config as I2cConfig, I2c},
    peripherals::{GPIO8, GPIO9, I2C0},
    time::Rate,
};
use static_cell::StaticCell;

use crate::Error;

/// The I2C bus: the keyboard, the ES8311 codec and the BMI270 share it.
///
/// Each transfer holds a critical section (interrupts off, the other core
/// kept out), so its devices can be used from any executor or core. That
/// delays interrupts by the transfer: about 0.3 ms for a key or motion
/// read, and 3 ms for each 128-byte burst of the motion sensor's start-up.
pub type I2cBus = Mutex<RefCell<I2c<'static, Blocking>>>;

/// One device's handle on the shared [`I2cBus`].
pub type I2cDevice = CriticalSectionDevice<'static, I2c<'static, Blocking>>;

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
    .with_scl(pins.scl);
    Ok(BUS.init(Mutex::new(RefCell::new(i2c))))
}
