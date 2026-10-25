//! Board support for the M5Stack Cardputer ADV on esp-hal.
//!
//! - [`display`]: the 1.14" ST7789V2 LCD as a 240×135 RGB565
//!   embedded-graphics `DrawTarget` (landscape, keyboard below), and its
//!   [`Backlight`], dimmable by PWM.
//! - [`keyboard`]: the 56-key keyboard (TCA8418 on I2C), with Shift/Fn layers
//!   from [`cardputer_adv_keyboard`]. [`Keyboard::wait_inputs`] sleeps until
//!   the TCA8418 signals a key event on its interrupt line.
//! - [`imu`]: the BMI270 motion sensor's accelerometer, in milli-g.
//! - [`speaker`]: the speaker, fed with samples over I2S (the ES8311 codec,
//!   then an NS4150B amplifier); [`enable_codec`] turns its DAC path on and
//!   [`set_volume`] sets its level.
//! - [`i2c`]: the I2C bus the keyboard, the codec and the motion sensor
//!   share. Only needed to use more than the keyboard; [`keyboard`] alone
//!   sets it up itself.
//! - [`battery`]: the battery voltage. The board can't tell whether it's
//!   charging.
//!
//! Pins (from the ADV schematic and working firmware):
//!
//! | Function | GPIO |
//! |---|---|
//! | LCD SCK / MOSI / CS / DC / RST | 36 / 35 / 37 / 34 / 33 |
//! | LCD backlight, PWM on LEDC (also powers the RGB LED) | 38 |
//! | I2C SDA / SCL (keyboard, ES8311, BMI270) | 8 / 9 |
//! | Keyboard interrupt | 11 |
//! | Battery voltage (ADC1, through a 2:1 divider) | 10 |
//! | I2S BCLK / WS / DOUT (to the ES8311; no MCLK) | 41 / 43 / 42 |

#![no_std]

use core::{cell::RefCell, convert::Infallible, fmt};

pub use cardputer_adv_keyboard as keys;
use critical_section::Mutex;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{IntoStorage, Rgb565},
    primitives::Rectangle,
};
use embedded_hal::digital::OutputPin;
use embedded_hal_bus::{
    i2c::CriticalSectionDevice,
    spi::{ExclusiveDevice, NoDelay},
};
use esp_hal::{
    Async, Blocking,
    delay::Delay,
    analog::adc::{Adc, AdcCalCurve, AdcConfig, AdcPin, Attenuation},
    gpio::{DriveMode, Input, InputConfig, Level, Output, OutputConfig, Pull},
    ledc::{
        LSGlobalClkSource, Ledc, LowSpeed,
        channel::{self, ChannelIFace},
        timer::{self, TimerIFace},
    },
    dma::DmaTxStreamBuf,
    dma_tx_stream_buffer,
    i2c::master::{Config as I2cConfig, I2c},
    i2s::master::{Channels, DataFormat, I2s, I2sTxDmaTransfer, TdmConfig},
    peripherals::{
        ADC1, DMA_CH0, GPIO8, GPIO9, GPIO10, GPIO11, GPIO33, GPIO34, GPIO35, GPIO36, GPIO37,
        GPIO38, GPIO41, GPIO42, GPIO43, I2C0, I2S0, LEDC, SPI2,
    },
    spi::{
        Mode,
        master::{AnySpi, Config as SpiConfig, Spi, SpiDma, SpiMasterDmaChannel},
    },
    time::Rate,
};
use mipidsi::{
    Builder,
    interface::{Interface, InterfacePixelFormat, SpiInterface},
    models::ST7789,
    options::{ColorInversion, Orientation, Rotation},
};
use static_cell::StaticCell;

/// Display width in landscape.
pub const WIDTH: u16 = 240;
/// Display height in landscape.
pub const HEIGHT: u16 = 135;

type DisplaySpi = ExclusiveDevice<Spi<'static, Blocking>, Output<'static>, NoDelay>;

/// The LCD: an embedded-graphics `DrawTarget<Color = Rgb565>`.
pub type Display =
    mipidsi::Display<SpiInterface<'static, DisplaySpi, Output<'static>>, ST7789, Output<'static>>;

/// The peripherals the LCD uses.
#[allow(missing_docs, reason = "the fields are the pins they're named after")]
pub struct DisplayPins {
    pub spi: SPI2<'static>,
    pub sck: GPIO36<'static>,
    pub mosi: GPIO35<'static>,
    pub cs: GPIO37<'static>,
    pub dc: GPIO34<'static>,
    pub reset: GPIO33<'static>,
    pub backlight: GPIO38<'static>,
    /// Drives the backlight with PWM, for dimming.
    pub ledc: LEDC<'static>,
}

/// What failed. The underlying driver errors aren't kept: on this board,
/// each part has one way to fail that matters (it didn't answer, or the
/// peripheral couldn't be configured).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The LCD's SPI bus couldn't be configured.
    Spi,
    /// The I2C bus couldn't be configured.
    I2c,
    /// The LCD didn't take its initialisation sequence.
    DisplayInit,
    /// The TCA8418 keyboard controller didn't answer.
    Keyboard,
    /// The backlight's PWM couldn't be configured.
    Backlight,
    /// The motion sensor wasn't found, or didn't start.
    Imu,
    /// The I2S output couldn't be started, or restarted after running dry.
    Speaker,
    /// The ES8311 codec didn't answer.
    Codec,
    /// A frame couldn't be sent to the LCD.
    DisplayWrite,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Spi => "LCD SPI bus setup failed",
            Self::I2c => "I2C bus setup failed",
            Self::DisplayInit => "LCD initialisation failed",
            Self::Keyboard => "keyboard controller (TCA8418) not answering",
            Self::Backlight => "backlight PWM setup failed",
            Self::Imu => "motion sensor (BMI270) not found or not starting",
            Self::Speaker => "speaker I2S stream failed",
            Self::Codec => "audio codec (ES8311) not answering",
            Self::DisplayWrite => "LCD frame write failed",
        })
    }
}

impl core::error::Error for Error {}

/// The LCD backlight, dimmable: PWM at 256 Hz from LEDC low-speed channel
/// 0, as M5Stack's M5GFX drives it on the Cardputer. Its LED driver can't
/// follow fast PWM: at 24 kHz, 15% duty looked off. GPIO38 also powers the
/// RGB LED, which dims with it.
pub struct Backlight {
    channel: channel::Channel<'static, LowSpeed>,
}

impl Backlight {
    /// Sets the brightness, 0 (off) to 100 (full).
    ///
    /// # Errors
    ///
    /// If LEDC rejects the duty (it can't, for 0–100).
    pub fn set_percent(&mut self, percent: u8) -> Result<(), Error> {
        self.channel
            .set_duty(percent.min(100))
            .map_err(|_| Error::Backlight)
    }
}

/// Starts the backlight at full brightness.
///
/// # Panics
///
/// If called twice (the PWM timer is a `static`).
fn backlight(ledc: LEDC<'static>, pin: GPIO38<'static>) -> Result<Backlight, Error> {
    static TIMER: StaticCell<esp_hal::ledc::timer::Timer<'static, LowSpeed>> = StaticCell::new();

    let mut ledc = Ledc::new(ledc);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let timer = TIMER.init(ledc.timer::<LowSpeed>(timer::Number::Timer0));
    timer
        .configure(timer::config::Config {
            // 80 MHz / (256 Hz × 1024 steps): a divider LEDC can do.
            duty: timer::config::Duty::Duty10Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_hz(256),
        })
        .map_err(|_| Error::Backlight)?;
    let mut channel = ledc.channel(channel::Number::Channel0, pin);
    channel
        .configure(channel::config::Config {
            timer,
            duty_pct: 100,
            drive_mode: DriveMode::PushPull,
        })
        .map_err(|_| Error::Backlight)?;
    Ok(Backlight { channel })
}

/// Initialises the LCD (landscape, cleared by the caller) and turns the
/// backlight on at full brightness.
///
/// # Errors
///
/// If the SPI bus or the panel can't be set up.
///
/// # Panics
///
/// If called twice (the SPI transfer buffer and the PWM timer are `static`).
pub fn display(pins: DisplayPins) -> Result<(Display, Backlight), Error> {
    static BUFFER: StaticCell<[u8; 512]> = StaticCell::new();

    let backlight = backlight(pins.ledc, pins.backlight)?;
    let dc = Output::new(pins.dc, Level::Low, OutputConfig::default());
    let reset = Output::new(pins.reset, Level::High, OutputConfig::default());
    let cs = Output::new(pins.cs, Level::High, OutputConfig::default());
    let spi = Spi::new(pins.spi, spi_config())
        .map_err(|_| Error::Spi)?
        .with_sck(pins.sck)
        .with_mosi(pins.mosi);
    let device = ExclusiveDevice::new_no_delay(spi, cs).map_err(|_| Error::Spi)?;
    let interface = SpiInterface::new(device, dc, BUFFER.init([0; 512]));
    let display = init_panel(interface, reset)?;
    Ok((display, backlight))
}

/// The LCD's SPI bus: 40 MHz, as M5GFX runs it.
fn spi_config() -> SpiConfig {
    SpiConfig::default()
        .with_frequency(Rate::from_mhz(40))
        .with_mode(Mode::_0)
}

/// Where the panel's 135×240 pixels sit in the ST7789's 240×320 memory.
const PANEL_OFFSET: (u16, u16) = (52, 40);

/// Initialises the panel in landscape (keyboard below), colours inverted.
fn init_panel<DI, RST>(interface: DI, reset: RST) -> Result<mipidsi::Display<DI, ST7789, RST>, Error>
where
    DI: Interface,
    Rgb565: InterfacePixelFormat<DI::Word>,
    RST: OutputPin,
{
    Builder::new(ST7789, interface)
        .reset_pin(reset)
        .display_size(HEIGHT, WIDTH)
        .display_offset(PANEL_OFFSET.0, PANEL_OFFSET.1)
        .orientation(Orientation::new().rotate(Rotation::Deg90))
        .invert_colors(ColorInversion::Inverted)
        .init(&mut Delay::new())
        .map_err(|_| Error::DisplayInit)
}

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
    #[allow(clippy::cast_possible_truncation, reason = "the width (240) fits")]
    const FULL: Self = Self {
        first: 0,
        last: (W - 1) as u8,
    };

    const fn is_clean(self) -> bool {
        self.first > self.last
    }

    /// The span, widened to take in column `x`.
    #[allow(clippy::cast_possible_truncation, reason = "x < WIDTH (240)")]
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
    #[allow(clippy::cast_possible_truncation, reason = "x < WIDTH (240)")]
    fn fill_row(&mut self, y: usize, x0: usize, x1: usize, raw: u16) {
        // Two pixels a store where the row is word-aligned: memory writes
        // are what filling costs.
        let row = &mut self.pixels[y * W + x0..=y * W + x1];
        let (head, words, tail) = bytemuck::pod_align_to_mut::<u16, u32>(row);
        head.fill(raw);
        words.fill(u32::from(raw) * 0x0001_0001);
        tail.fill(raw);
        let span = &mut self.written[y];
        span.first = span.first.min(x0 as u8);
        span.last = span.last.max(x1 as u8);
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
            while let Some(hash) = (y < H).then(|| frame.changed(y)).flatten() {
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

    /// Selects the rectangle the next pixels fill, row by row.
    #[allow(clippy::cast_possible_truncation, reason = "inside the 240×135 frame")]
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
#[allow(missing_docs, reason = "the fields are the pins they're named after")]
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

/// The peripherals the keyboard uses.
pub struct KeyboardPins {
    /// The I2C bus it shares (set up by [`keyboard`]).
    pub i2c: I2C0<'static>,
    /// I2C data.
    pub sda: GPIO8<'static>,
    /// I2C clock.
    pub scl: GPIO9<'static>,
    /// The TCA8418's interrupt output (active low).
    pub interrupt: GPIO11<'static>,
}

/// How often to look for key events anyway, in case an interrupt is missed
/// (or the line doesn't work): key presses are then late, not lost.
const FALLBACK_POLL: Duration = Duration::from_secs(1);

/// The pause after a wake-up with nothing to read. If the line is stuck low
/// this turns a busy loop into slow polling, so other tasks keep running.
const SPURIOUS_BACKOFF: Duration = Duration::from_millis(10);

/// The most key inputs one read returns (the TCA8418's FIFO holds 10
/// events).
const MAX_INPUTS: usize = 10;

/// The keyboard: the TCA8418 driver and its interrupt line.
pub struct Keyboard {
    keys: keys::Keyboard<I2cDevice>,
    interrupt: Input<'static>,
    /// Events arrived while the last batch was being read: read again
    /// without waiting.
    pending: bool,
    missed: u32,
}

/// Why [`Keyboard::wait_inputs`] went to read the keyboard.
enum Wake {
    Interrupt,
    Fallback,
    Pending,
}

impl Keyboard {
    /// Waits for key presses and returns them (with Shift/Fn applied).
    ///
    /// Sleeps until the TCA8418 pulls its interrupt line low, so an idle
    /// keyboard costs nothing. It waits for the low *level*, not an edge:
    /// events that arrive while the caller is busy keep the line low.
    ///
    /// # Errors
    ///
    /// If the TCA8418 can't be read over I2C.
    pub async fn wait_inputs(&mut self) -> Result<impl Iterator<Item = keys::KeyInput>, Error> {
        loop {
            let wake = if core::mem::take(&mut self.pending) {
                Wake::Pending
            } else {
                match select(self.interrupt.wait_for_low(), Timer::after(FALLBACK_POLL)).await {
                    Either::First(()) => Wake::Interrupt,
                    Either::Second(()) => Wake::Fallback,
                }
            };
            let events = self.keys.events().map_err(|_| Error::Keyboard)?;
            // Clear every flag (key events, overflow, …) so no source keeps
            // the line low, then catch an event that landed in between. The
            // keys are already read, so a failure here mustn't lose them:
            // read again on the next call instead.
            let cleared = self.keys.clear_interrupts().is_ok();
            self.pending = !cleared || self.keys.events_available().unwrap_or(true);

            let mut any_event = false;
            let mut inputs = [None; MAX_INPUTS];
            for (slot, input) in inputs.iter_mut().zip(events.filter_map(|event| {
                any_event = true;
                event.input
            })) {
                *slot = Some(input);
            }
            if !any_event {
                // Woken with nothing to read: back off, so a line stuck low
                // can't keep this task from yielding.
                if matches!(wake, Wake::Interrupt) {
                    Timer::after(SPURIOUS_BACKOFF).await;
                }
                continue;
            }
            if matches!(wake, Wake::Fallback) {
                // Events without an interrupt: the line isn't working.
                self.missed = self.missed.saturating_add(1);
            }
            // Releases (and lone modifiers) are events without an input.
            if inputs[0].is_none() {
                continue;
            }
            return Ok(inputs.into_iter().flatten());
        }
    }

    /// How many times key events were found only by the fallback poll, which
    /// points at a broken interrupt line.
    #[must_use]
    pub const fn missed_interrupts(&self) -> u32 {
        self.missed
    }

    /// The underlying driver, for anything [`wait_inputs`](Self::wait_inputs)
    /// doesn't cover.
    pub const fn driver(&mut self) -> &mut keys::Keyboard<I2cDevice> {
        &mut self.keys
    }
}

/// Initialises the I2C bus, the keyboard and its interrupt. To use the
/// motion sensor too, set the bus up with [`i2c`] and use
/// [`keyboard_on`] instead.
///
/// # Errors
///
/// If the bus or the TCA8418 can't be set up.
///
/// # Panics
///
/// If the bus was already set up (by an earlier call, or [`i2c`]).
pub fn keyboard(pins: KeyboardPins) -> Result<Keyboard, Error> {
    let bus = i2c(I2cPins {
        i2c: pins.i2c,
        sda: pins.sda,
        scl: pins.scl,
    })?;
    keyboard_on(bus, pins.interrupt)
}

/// Tries at setting up the TCA8418: right after a flash or a reset it can
/// miss the first transfers.
const KEYBOARD_TRIES: u32 = 5;
/// The wait between those tries.
const KEYBOARD_RETRY_MS: u32 = 20;

/// Initialises the keyboard on the shared bus, and its interrupt.
///
/// # Errors
///
/// If the TCA8418 can't be set up, after [`KEYBOARD_TRIES`] tries.
pub fn keyboard_on(bus: &'static I2cBus, interrupt: GPIO11<'static>) -> Result<Keyboard, Error> {
    let set_up = || -> Result<keys::Keyboard<I2cDevice>, Error> {
        let mut keys =
            keys::Keyboard::new(CriticalSectionDevice::new(bus)).map_err(|_| Error::Keyboard)?;
        keys.enable_keyboard_interrupt()
            .map_err(|_| Error::Keyboard)?;
        keys.clear_interrupts().map_err(|_| Error::Keyboard)?;
        Ok(keys)
    };
    let mut keys = set_up();
    for _ in 1..KEYBOARD_TRIES {
        if keys.is_ok() {
            break;
        }
        Delay::new().delay_millis(KEYBOARD_RETRY_MS);
        keys = set_up();
    }
    let keys = keys?;
    // The TCA8418's INT is open-drain.
    let interrupt = Input::new(interrupt, InputConfig::default().with_pull(Pull::Up));
    Ok(Keyboard {
        keys,
        interrupt,
        pending: false,
        missed: 0,
    })
}

/// The largest I2C write while uploading the BMI270's configuration.
const IMU_BURST: u16 = 128;

/// The BMI270 driver on the shared bus.
type Bmi270 = bmi2::Bmi2<bmi2::interface::I2cInterface<I2cDevice>, Delay, { IMU_BURST as usize }>;

/// The BMI270's accelerometer: ±8 g at 100 Hz (its gyroscope stays off).
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
}

/// Starts the BMI270 on the shared bus: finds it at either of its addresses,
/// uploads its configuration (8 kB, about 50 ms) and turns the accelerometer
/// on.
///
/// # Errors
///
/// If no BMI270 answers, or it doesn't start.
pub fn imu(bus: &'static I2cBus) -> Result<Imu, Error> {
    use bmi2::{
        I2cAddr, config,
        types::{AccBwp, AccConf, AccRange, Burst, Odr, PerfMode, PwrCtrl},
    };
    let mut found = None;
    for address in [I2cAddr::Default, I2cAddr::Alternative] {
        let mut bmi = Bmi270::new_i2c(
            CriticalSectionDevice::new(bus),
            Delay::new(),
            address,
            Burst::new(IMU_BURST),
        );
        if bmi.get_chip_id().is_ok_and(|id| id == 0x24) {
            found = Some(bmi);
            break;
        }
    }
    let mut bmi = found.ok_or(Error::Imu)?;
    bmi.init(&config::BMI270_CONFIG_FILE)
        .map_err(|_| Error::Imu)?;
    bmi.set_acc_range(AccRange::Range8g).map_err(|_| Error::Imu)?;
    bmi.set_acc_conf(AccConf {
        odr: Odr::Odr100,
        bwp: AccBwp::NormAvg4,
        filter_perf: PerfMode::Perf,
    })
    .map_err(|_| Error::Imu)?;
    bmi.set_pwr_ctrl(PwrCtrl {
        aux_en: false,
        gyr_en: false,
        acc_en: true,
        temp_en: false,
    })
    .map_err(|_| Error::Imu)?;
    Ok(Imu { bmi })
}

/// The speaker's sample rate.
pub const SAMPLE_RATE: u32 = 44_100;

/// The DMA buffer: 4096 bytes is 1024 stereo frames, 23 ms, in 8 chunks.
/// Samples are pushed at most that far ahead of being heard.
const SPEAKER_BUFFER: usize = 4096;
const SPEAKER_CHUNK: usize = 512;

/// The peripherals the speaker uses.
#[allow(missing_docs, reason = "the fields are the pins they're named after")]
pub struct SpeakerPins {
    pub i2s: I2S0<'static>,
    pub dma: DMA_CH0<'static>,
    pub bclk: GPIO41<'static>,
    pub ws: GPIO43<'static>,
    pub dout: GPIO42<'static>,
}

type SpeakerTransfer = I2sTxDmaTransfer<'static, Async, DmaTxStreamBuf>;

/// Bytes of silence pushed at a time when (re)starting the stream.
const SILENCE: [u8; 64] = [0; 64];

/// The speaker: an I2S stream to the ES8311 codec, playing silence until
/// [`run`](Self::run) feeds it.
pub struct Speaker {
    /// `None` only while restarting.
    transfer: Option<SpeakerTransfer>,
}

impl Speaker {
    /// Plays the mono samples `fill` writes, at [`SAMPLE_RATE`], forever.
    ///
    /// `fill` is asked for as many as the buffer has room for, every few
    /// milliseconds. If it falls behind by more than the buffer (23 ms),
    /// the DMA stops; the stream then restarts on silence, so the sound
    /// skips rather than stops. To keep up while other tasks hold the CPU
    /// for long (drawing a frame, say), run this on a higher-priority
    /// executor, such as esp-rtos's `InterruptExecutor`.
    ///
    /// # Errors
    ///
    /// Returns only if the stream couldn't be restarted.
    pub async fn run(&mut self, mut fill: impl FnMut(&mut [i16])) -> Result<Infallible, Error> {
        let mut mono = [0i16; 256];
        loop {
            let transfer = self.transfer.as_mut().ok_or(Error::Speaker)?;
            if transfer.wait_for_available_async().await.is_err() {
                self.restart()?;
                continue;
            }
            loop {
                let pushed = transfer.push_with(|bytes| {
                    // Stereo frames, the same sample left and right. The
                    // chunks are multiples of 4 bytes, so a frame never
                    // straddles two, and while there is room, there is
                    // room for a whole frame.
                    debug_assert!(bytes.is_empty() || bytes.len() >= 4);
                    let frames = (bytes.len() / 4).min(mono.len());
                    fill(&mut mono[..frames]);
                    for (frame, sample) in bytes.chunks_exact_mut(4).zip(&mono[..frames]) {
                        let [lo, hi] = sample.to_le_bytes();
                        frame.copy_from_slice(&[lo, hi, lo, hi]);
                    }
                    frames * 4
                });
                if pushed == 0 {
                    break;
                }
            }
        }
    }

    /// Starts the stream again after it ran dry: a fresh buffer of silence
    /// on the same memory. The bit clock, and so the codec's clock, stopped
    /// with it; the codec carries on when the clock does.
    fn restart(&mut self) -> Result<(), Error> {
        let transfer = self.transfer.take().ok_or(Error::Speaker)?;
        let (tx, buffer) = transfer.stop();
        let (descriptors, bytes) = buffer.split();
        let mut buffer = DmaTxStreamBuf::new(descriptors, bytes).map_err(|_| Error::Speaker)?;
        while buffer.push(&SILENCE) > 0 {}
        self.transfer = Some(tx.write(buffer).map_err(|_| Error::Speaker)?);
        Ok(())
    }
}

/// Starts the I2S stream to the codec (16-bit stereo at [`SAMPLE_RATE`]),
/// playing silence. The codec takes its clock from it, so call
/// [`enable_codec`] once this is running.
///
/// # Errors
///
/// If the I2S peripheral or its DMA can't be set up.
///
/// # Panics
///
/// If called twice (its DMA buffer is static).
pub fn speaker(pins: SpeakerPins) -> Result<Speaker, Error> {
    let i2s = I2s::new(
        pins.i2s,
        pins.dma,
        TdmConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(SAMPLE_RATE))
            .with_data_format(DataFormat::Data16Channel16)
            .with_channels(Channels::STEREO),
    )
    .map_err(|_| Error::Speaker)?
    .into_async();
    let tx = i2s
        .i2s_tx
        .with_bclk(pins.bclk)
        .with_ws(pins.ws)
        .with_dout(pins.dout)
        .build();
    let mut buffer = dma_tx_stream_buffer!(SPEAKER_BUFFER, SPEAKER_CHUNK);
    // Start full of silence.
    while buffer.push(&SILENCE) > 0 {}
    let transfer = tx.write(buffer).map_err(|_| Error::Speaker)?;
    Ok(Speaker {
        transfer: Some(transfer),
    })
}

/// The ES8311's I2C address.
const ES8311: u8 = 0x18;

/// Turns on the ES8311's DAC path to the amplifier, at 0 dB. Its master
/// clock comes from the I2S bit clock (the board has no MCLK line), so the
/// [`speaker`] stream must be running. The sequence is M5Unified's Cardputer
/// ADV speaker enable, as espp's driver uses it.
///
/// # Errors
///
/// If the codec doesn't answer.
pub fn enable_codec(bus: &'static I2cBus) -> Result<(), Error> {
    use embedded_hal::i2c::I2c as _;
    const INIT: [[u8; 2]; 8] = [
        [0x00, 0x80], // reset: power on, state machine enabled
        [0x01, 0xB5], // clock manager: MCLK from BCLK, DAC clocks on
        [0x02, 0x18], // clock manager: MCLK = BCLK × 8 (256 fs at 32 bits a frame)
        [0x0D, 0x01], // system: analog circuits on
        [0x12, 0x00], // system: DAC on
        [0x13, 0x10], // system: output to the amplifier on
        [ES8311_DAC_VOLUME, 0xBF], // DAC volume: 0 dB
        [0x37, 0x08], // DAC equalizer bypassed
    ];
    let mut codec = CriticalSectionDevice::new(bus);
    for write in INIT {
        codec.write(ES8311, &write).map_err(|_| Error::Codec)?;
    }
    Ok(())
}

/// The ES8311's DAC volume register: 0x00 is −95.5 dB, 0xBF is 0 dB, in
/// 0.5 dB steps.
const ES8311_DAC_VOLUME: u8 = 0x32;

/// Sets the speaker's level, 0 (silent, −95.5 dB) to 100 (0 dB, the level
/// [`enable_codec`] starts at), evenly in decibels. The codec must be on.
///
/// # Errors
///
/// If the codec doesn't answer.
pub fn set_volume(bus: &'static I2cBus, percent: u8) -> Result<(), Error> {
    use embedded_hal::i2c::I2c as _;
    let step = u16::from(percent.min(100)) * 0xBF / 100;
    let register = u8::try_from(step).unwrap_or(0xBF);
    CriticalSectionDevice::new(bus)
        .write(ES8311, &[ES8311_DAC_VOLUME, register])
        .map_err(|_| Error::Codec)
}

/// The peripherals the battery measurement uses.
pub struct BatteryPins {
    /// The ADC the battery is read with.
    pub adc: ADC1<'static>,
    /// The battery, through the board's 2:1 divider.
    pub pin: GPIO10<'static>,
}

/// How many readings [`Battery::millivolts`] averages.
const BATTERY_SAMPLES: u32 = 16;

/// The battery voltage, measured as M5Stack's M5Unified does for the
/// Cardputer and Cardputer ADV: GPIO10 on ADC1, through a 2:1 divider.
pub struct Battery {
    adc: Adc<'static, ADC1<'static>, Blocking>,
    pin: AdcPin<GPIO10<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
}

impl Battery {
    /// The battery voltage in millivolts, averaged over a few readings.
    ///
    /// The board can't tell whether it's charging: on USB with the power
    /// switch on, this is the charging voltage, close to full.
    pub fn millivolts(&mut self) -> u16 {
        let sum: u32 = (0..BATTERY_SAMPLES)
            .map(|_| u32::from(self.adc.read_blocking(&mut self.pin)))
            .sum();
        u16::try_from(sum / BATTERY_SAMPLES * 2).unwrap_or(u16::MAX)
    }
}

/// Sets up the battery measurement: ADC1 with the chip's calibration curve,
/// at 11 dB attenuation (up to about 3.1 V at the pin, 6.2 V at the battery).
#[must_use]
pub fn battery(pins: BatteryPins) -> Battery {
    let mut config = AdcConfig::new();
    let pin = config.enable_pin_with_cal::<_, AdcCalCurve<ADC1<'static>>>(
        pins.pin,
        Attenuation::_11dB,
    );
    Battery {
        adc: Adc::new(pins.adc, config),
        pin,
    }
}
