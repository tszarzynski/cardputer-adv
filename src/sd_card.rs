//! The microSD card, on its own SPI bus, through embedded-sdmmc.

use core::{cell::Cell, fmt};

use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx, SdCardError};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
    peripherals::{GPIO12, GPIO14, GPIO39, GPIO40, SPI3},
    spi::{
        Mode,
        master::{Config as SpiConfig, Spi},
    },
    time::Rate,
};

use crate::Error;

/// embedded-sdmmc's driver, on the microSD card's SPI bus.
type Card =
    embedded_sdmmc::SdCard<ExclusiveDevice<Spi<'static, Blocking>, Output<'static>, Delay>, Delay>;

/// The peripherals the microSD card uses.
#[expect(missing_docs, reason = "the fields are the pins they're named after")]
#[derive(Debug)]
pub struct SdCardPins {
    pub spi: SPI3<'static>,
    pub sck: GPIO40<'static>,
    pub mosi: GPIO14<'static>,
    pub miso: GPIO39<'static>,
    pub cs: GPIO12<'static>,
}

/// The SPI clock while the card starts up (SD cards must take 400 kHz),
/// and after.
const SD_INIT_CLOCK: Rate = Rate::from_khz(400);
const SD_CLOCK: Rate = Rate::from_mhz(20);

/// The microSD card: an embedded-sdmmc [`BlockDevice`], for reading and
/// writing FAT files through an [`embedded_sdmmc::VolumeManager`] (the
/// crate is re-exported, so its version matches).
///
/// The card is started on first use, at 400 kHz, and then run at 20 MHz. A
/// failed access starts it again on the next, so a card inserted after
/// [`sd_card`], or swapped, is picked up.
///
/// It's blocking: a long read or write holds up the executor it's called
/// on, keys and all.
pub struct SdCard {
    card: Card,
    /// Whether the card has been started, and the bus sped up.
    started: Cell<bool>,
}

impl SdCard {
    /// Starts the card if it isn't, then runs `access` on it. If that fails,
    /// the card is started again next time.
    fn access<T>(
        &self,
        access: impl FnOnce(&Card) -> Result<T, SdCardError>,
    ) -> Result<T, SdCardError> {
        if !self.started.get() {
            self.start()?;
        }
        access(&self.card).inspect_err(|_| self.started.set(false))
    }

    /// Starts the card: at least 74 clocks with its CS high, its start-up
    /// commands at 400 kHz, then the bus at full speed.
    fn start(&self) -> Result<(), SdCardError> {
        self.set_clock(SD_INIT_CLOCK)?;
        self.card
            .spi(|device| device.bus_mut().write(&[0xFF; 10]))
            .map_err(|_| SdCardError::Transport)?;
        self.card.mark_card_uninit();
        self.card.num_bytes()?;
        self.set_clock(SD_CLOCK)?;
        self.started.set(true);
        Ok(())
    }

    fn set_clock(&self, clock: Rate) -> Result<(), SdCardError> {
        self.card
            .spi(|device| device.bus_mut().apply_config(&spi_config(clock)))
            .map_err(|_| SdCardError::Transport)
    }
}

impl BlockDevice for SdCard {
    type Error = SdCardError;

    fn read(&self, blocks: &mut [Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        self.access(|card| card.read(blocks, start_block_idx))
    }

    fn write(&self, blocks: &[Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        self.access(|card| card.write(blocks, start_block_idx))
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        self.access(BlockDevice::num_blocks)
    }
}

impl fmt::Debug for SdCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SdCard")
            .field("started", &self.started.get())
            .finish_non_exhaustive()
    }
}

fn spi_config(clock: Rate) -> SpiConfig {
    SpiConfig::default()
        .with_frequency(clock)
        .with_mode(Mode::_0)
}

/// Sets up the microSD card's SPI bus. Nothing is sent to the card until
/// the [`SdCard`] is first used, so there needn't be one in yet.
///
/// # Errors
///
/// [`Error::SdCard`] if the SPI bus can't be set up.
pub fn sd_card(pins: SdCardPins) -> Result<SdCard, Error> {
    let spi = Spi::new(pins.spi, spi_config(SD_INIT_CLOCK))
        .map_err(|_| Error::SdCard)?
        .with_sck(pins.sck)
        .with_mosi(pins.mosi)
        .with_miso(pins.miso);
    let cs = Output::new(pins.cs, Level::High, OutputConfig::default());
    let device = ExclusiveDevice::new(spi, cs, Delay::new()).map_err(|_| Error::SdCard)?;
    Ok(SdCard {
        card: Card::new(device, Delay::new()),
        started: Cell::new(false),
    })
}
