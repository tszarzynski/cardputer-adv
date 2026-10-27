//! The LCD fed from a framebuffer by DMA.

use core::{convert::Infallible, fmt};

use embassy_time::{Duration, Timer};
use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{IntoStorage, Rgb565},
    primitives::Rectangle,
};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    Async,
    gpio::{Level, Output, OutputConfig},
    spi::master::{AnySpi, Spi, SpiDma, SpiMasterDmaChannel},
};
use mipidsi::interface::SpiInterface;

use crate::{
    Backlight, DisplayPins, Error, HEIGHT, WIDTH,
    display::{PANEL_OFFSET, backlight, init_panel, spi_config},
};

const W: usize = WIDTH as usize;
const H: usize = HEIGHT as usize;

/// The columns of a row written since the last flush, `first..=last`.
/// Clean when `first > last`.
#[derive(Clone, Copy)]
struct Span {
    first: u8,
    last: u8,
}

impl Span {
    const CLEAN: Self = Self {
        first: u8::MAX,
        last: 0,
    };
    #[expect(clippy::cast_possible_truncation, reason = "the width (240) fits")]
    const FULL: Self = Self {
        first: 0,
        last: (W - 1) as u8,
    };

    const fn is_clean(self) -> bool {
        self.first > self.last
    }

    /// The span, widened to take in column `x`.
    #[expect(clippy::cast_possible_truncation, reason = "x < WIDTH (240)")]
    fn with(self, x: usize) -> Self {
        let x = x as u8;
        Self {
            first: self.first.min(x),
            last: self.last.max(x),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            first: self.first.min(other.first),
            last: self.last.max(other.last),
        }
    }
}

/// A row's fingerprint: tells whether it differs from the row last sent.
/// Each step is a bijection of the running value, so a change to one
/// pair of pixels always shows; two changes cancelling out is a 1 in 2³²
/// chance.
fn row_hash(row: &[u16]) -> u32 {
    let (pairs, _) = row.as_chunks::<2>();
    pairs.iter().fold(0x811C_9DC5, |hash, &[a, b]| {
        (hash ^ (u32::from(a) | u32::from(b) << 16))
            .wrapping_mul(0x0100_0193)
            .rotate_left(5)
    })
}

/// The whole screen in memory (64.8 kB of RGB565), for [`FrameDisplay`].
///
/// Draw into it with embedded-graphics, overlapping shapes freely; then
/// [`FrameDisplay::flush`] sends only the rows that changed since the last
/// flush. Rows are compared with what was sent, so redrawing everything
/// each frame is fine: rows that come out the same aren't sent again.
///
/// It's too big for a task's stack; keep it in a static:
///
/// ```ignore
/// static FRAME: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());
/// ```
pub struct Frame {
    /// RGB565, row by row, each stored so that its bytes in memory are
    /// big-endian: the bytes the panel takes. Whole `u16`s, so filling
    /// takes one store a pixel.
    pixels: [u16; W * H],
    /// What each row has had drawn on it since the last flush.
    written: [Span; H],
    /// Each row's [`row_hash`] when it was last sent.
    sent: [u32; H],
    /// Send every written row, whatever its hash: the panel's contents
    /// aren't known.
    stale: bool,
}

impl Frame {
    /// A black frame, all of it still to be sent (the panel starts with
    /// whatever its memory held).
    #[must_use]
    #[expect(
        clippy::large_stack_arrays,
        reason = "made at compile time, into a static (see above)"
    )]
    pub const fn new() -> Self {
        Self {
            pixels: [0; W * H],
            written: [Span::FULL; H],
            sent: [0; H],
            stale: true,
        }
    }

    /// Marks the whole frame to be sent again, for when the panel's
    /// contents were lost or overwritten.
    pub const fn invalidate(&mut self) {
        self.written = [Span::FULL; H];
        self.stale = true;
    }

    /// Row `y`'s hash, if it has to be sent: it was written, and it differs
    /// from what was sent. A row written back the way it was is marked
    /// clean.
    fn changed(&mut self, y: usize) -> Option<u32> {
        if self.written[y].is_clean() {
            return None;
        }
        let hash = row_hash(&self.pixels[y * W..(y + 1) * W]);
        if !self.stale && hash == self.sent[y] {
            self.written[y] = Span::CLEAN;
            return None;
        }
        Some(hash)
    }

    /// `color` as [`pixels`](Self::pixels) stores it.
    fn raw(color: Rgb565) -> u16 {
        u16::from_ne_bytes(color.into_storage().to_be_bytes())
    }

    /// Sets row `y`'s pixels `x0..=x1` to `raw`.
    fn fill_row(&mut self, y: usize, x0: usize, x1: usize, raw: u16) {
        // Two pixels a store where the row is word-aligned: memory writes
        // are what filling costs.
        let row = &mut self.pixels[y * W + x0..=y * W + x1];
        let (head, words, tail) = bytemuck::pod_align_to_mut::<u16, u32>(row);
        head.fill(raw);
        words.fill(u32::from(raw) * 0x0001_0001);
        tail.fill(raw);
        self.written[y] = self.written[y].with(x0).with(x1);
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(WIDTH.into(), HEIGHT.into())
    }
}

impl DrawTarget for Frame {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Infallible>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        for Pixel(point, color) in pixels {
            if let (Ok(x), Ok(y)) = (usize::try_from(point.x), usize::try_from(point.y))
                && x < W
                && y < H
            {
                self.pixels[y * W + x] = Self::raw(color);
                self.written[y] = self.written[y].with(x);
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Infallible> {
        // Clipped to the frame: `start..end` in each direction.
        let clip = |start: i32, length: u32, limit: usize| {
            let end = start.saturating_add_unsigned(length);
            let clamp = |v: i32| usize::try_from(v).unwrap_or(0).min(limit);
            (clamp(start), clamp(end))
        };
        let (x0, x1) = clip(area.top_left.x, area.size.width, W);
        let (y0, y1) = clip(area.top_left.y, area.size.height, H);
        if x0 >= x1 {
            return Ok(());
        }
        let raw = Self::raw(color);
        for y in y0..y1 {
            self.fill_row(y, x0, x1 - 1, raw);
        }
        Ok(())
    }
}

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
        let mut hashes = [0; H];
        let mut y = 0;
        while y < H {
            let top = y;
            let mut span = Span::CLEAN;
            while y < H
                && let Some(hash) = frame.changed(y)
            {
                hashes[y] = hash;
                span = span.union(frame.written[y]);
                y += 1;
            }
            if span.is_clean() {
                y += 1;
                continue;
            }
            let (first, last) = (usize::from(span.first), usize::from(span.last));
            self.window(first, top, last, y - 1).await?;
            if first == 0 && last == W - 1 {
                // Whole rows: one transfer.
                self.write(bytemuck::cast_slice(&frame.pixels[top * W..y * W]))
                    .await?;
            } else {
                for row in top..y {
                    let pixels = &frame.pixels[row * W + first..=row * W + last];
                    self.write(bytemuck::cast_slice(pixels)).await?;
                }
            }
            frame.sent[top..y].copy_from_slice(&hashes[top..y]);
            frame.written[top..y].fill(Span::CLEAN);
        }
        frame.stale = false;
        Ok(())
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
    .map_err(|_| Error::Spi)?
    .with_sck(pins.sck)
    .with_mosi(pins.mosi)
    .with_dma(dma);
    // mipidsi sets the panel up through borrows, which then end: the
    // flushes drive it directly.
    let mut buffer = [0; 64];
    let device = ExclusiveDevice::new_no_delay(&mut spi, &mut cs).map_err(|_| Error::Spi)?;
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

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let written = self.written.iter().filter(|span| !span.is_clean()).count();
        f.debug_struct("Frame")
            .field("rows_written", &written)
            .field("stale", &self.stale)
            .finish_non_exhaustive()
    }
}
