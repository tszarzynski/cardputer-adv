//! The LCD fed from a [`Frame`] by DMA.

use embassy_time::{Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    Async,
    gpio::{Level, Output, OutputConfig},
    spi::master::{AnySpi, Spi, SpiDma, SpiMasterDmaChannel},
};
use mipidsi::interface::SpiInterface;

use crate::{
    Backlight, DisplayPins, Error, Frame, HEIGHT,
    display::{backlight, init_panel, spi_config},
    frame::Run,
    panel::PANEL_OFFSET,
};

/// The LCD, fed from a [`Frame`] by DMA: sending a whole frame takes about
/// 13 ms (64.8 kB at 40 MHz), during which other tasks run.
#[derive(Debug)]
pub struct FrameDisplay {
    spi: SpiDma<'static, Async>,
    cs: Output<'static>,
    dc: Output<'static>,
    /// Held high: the panel resets while it's low.
    _reset: Output<'static>,
}

/// Transfers shorter than this go by the CPU, not DMA. Commands are a few
/// bytes, and may be in flash, where DMA can't read.
const MIN_DMA_TRANSFER: usize = 64;

/// The ST7789 commands [`FrameDisplay`] sends.
const SLPIN: u8 = 0x10;
const SLPOUT: u8 = 0x11;
const CASET: u8 = 0x2A;
const RASET: u8 = 0x2B;
const RAMWR: u8 = 0x2C;

/// The landscape frame's origin in the ST7789's memory, as mipidsi works it
/// out for a 90° rotation: columns mirrored, then swapped with rows.
const COLUMN_OFFSET: u16 = PANEL_OFFSET.1;
const ROW_OFFSET: u16 = 240 - (HEIGHT + PANEL_OFFSET.0);

impl FrameDisplay {
    /// Sends the rows of `frame` that changed since the last flush, each run
    /// of changed rows as one rectangle, as wide as what was drawn on them.
    ///
    /// If this is cancelled, the rows not yet sent are sent next time.
    ///
    /// # Errors
    ///
    /// If the SPI transfer fails; the rows not sent stay changed.
    pub async fn flush(&mut self, frame: &mut Frame) -> Result<(), Error> {
        self.cs.set_low();
        let sent = self.send(frame).await;
        self.cs.set_high();
        sent
    }

    async fn send(&mut self, frame: &mut Frame) -> Result<(), Error> {
        let mut from = 0;
        while let Some(run) = frame.next_run(from) {
            self.send_run(frame, &run).await?;
            frame.mark_sent(&run);
            from = run.rows.end;
        }
        frame.mark_shown();
        Ok(())
    }

    async fn send_run(&mut self, frame: &Frame, run: &Run) -> Result<(), Error> {
        let (first, last) = (*run.columns.start(), *run.columns.end());
        self.window(first, run.rows.start, last, run.rows.end - 1)
            .await?;
        if run.is_full_width() {
            // Whole rows: one transfer.
            self.write(frame.rows_bytes(run.rows.clone())).await
        } else {
            for row in run.rows.clone() {
                self.write(frame.row_bytes(row, run.columns.clone()))
                    .await?;
            }
            Ok(())
        }
    }

    /// Puts the panel's controller to sleep (`asleep`), keeping the picture
    /// in its memory, or wakes it. Asleep it draws a few microamps instead
    /// of a few milliamps; turn the [`Backlight`] off first, as the panel
    /// shows nothing meanwhile.
    ///
    /// Returns once the controller can take the next command (5 ms). It
    /// shouldn't be put back to sleep within 120 ms of waking.
    ///
    /// # Errors
    ///
    /// If the SPI transfer fails.
    pub async fn set_asleep(&mut self, asleep: bool) -> Result<(), Error> {
        self.cs.set_low();
        let sent = self.command(if asleep { SLPIN } else { SLPOUT }, &[]).await;
        self.cs.set_high();
        Timer::after(Duration::from_millis(5)).await;
        sent
    }

    /// Selects the rectangle the next pixels fill, row by row.
    #[expect(clippy::cast_possible_truncation, reason = "inside the 240×135 frame")]
    async fn window(&mut self, x0: usize, y0: usize, x1: usize, y1: usize) -> Result<(), Error> {
        let range = |from: usize, to: usize, offset: u16| {
            let [a, b] = (from as u16 + offset).to_be_bytes();
            let [c, d] = (to as u16 + offset).to_be_bytes();
            [a, b, c, d]
        };
        self.command(CASET, &range(x0, x1, COLUMN_OFFSET)).await?;
        self.command(RASET, &range(y0, y1, ROW_OFFSET)).await?;
        self.command(RAMWR, &[]).await
    }

    async fn command(&mut self, command: u8, args: &[u8]) -> Result<(), Error> {
        self.dc.set_low();
        self.write(&[command]).await?;
        self.dc.set_high();
        self.write(args).await
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        // Returns once the bytes are out, so DC can change after it.
        self.spi
            .write_async(bytes)
            .await
            .map_err(|_| Error::DisplayWrite)
    }
}

/// Initialises the LCD to be fed from a [`Frame`] by DMA, on the DMA channel
/// given, and turns the backlight on at full brightness.
///
/// # Errors
///
/// If the SPI bus or the panel can't be set up.
///
/// # Panics
///
/// If called twice, or with [`display`] (the PWM timer is a `static`).
///
/// [`display`]: crate::display
pub fn frame_display(
    pins: DisplayPins,
    dma: impl SpiMasterDmaChannel<'static, AnySpi<'static>>,
) -> Result<(FrameDisplay, Backlight), Error> {
    let backlight = backlight(pins.ledc, pins.backlight)?;
    let mut dc = Output::new(pins.dc, Level::Low, OutputConfig::default());
    let mut reset = Output::new(pins.reset, Level::High, OutputConfig::default());
    let mut cs = Output::new(pins.cs, Level::High, OutputConfig::default());
    let mut spi = Spi::new(
        pins.spi,
        spi_config().with_min_async_transfer_size(MIN_DMA_TRANSFER),
    )
    .map_err(|_| Error::DisplaySpi)?
    .with_sck(pins.sck)
    .with_mosi(pins.mosi)
    .with_dma(dma);
    // mipidsi sets the panel up through borrows, which then end: the
    // flushes drive it directly.
    let mut buffer = [0; 64];
    let device = ExclusiveDevice::new_no_delay(&mut spi, &mut cs).map_err(|_| Error::DisplaySpi)?;
    init_panel(SpiInterface::new(device, &mut dc, &mut buffer), &mut reset)?;
    Ok((
        FrameDisplay {
            spi: spi.into_async(),
            cs,
            dc,
            _reset: reset,
        },
        backlight,
    ))
}
