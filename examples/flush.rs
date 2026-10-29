//! Times `FrameDisplay::flush` on a Cardputer ADV, for changed areas of
//! different shapes, and logs each over USB: the mean time a flush takes,
//! and the bytes it sends a second.
//!
//! A run of changed rows narrower than the screen goes as one transfer a
//! row; a full-width run, as one transfer. The tall cases compare the two
//! for the same rows, to show what the per-row transfers cost.
//!
//! `just flush`, or `cargo run --release --example flush`

#![no_std]
#![no_main]
#![expect(
    clippy::future_not_send,
    reason = "esp-hal's async drivers aren't Send; each task runs on the executor that made it"
)]
#![expect(
    clippy::used_underscore_binding,
    reason = "esp-rtos's main macro passes the spawner on, which nothing here uses"
)]

use cardputer_adv::{Frame, FrameDisplay, HEIGHT, WIDTH};
use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, time::Instant, timer::timg::TimerGroup};
use log::info;
use static_cell::ConstStaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

static FRAME: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());

/// Flushes timed for each case.
const FLUSHES: u32 = 50;

/// The changed areas timed: a name, and the rectangle redrawn each flush.
const CASES: [(&str, i32, i32, u32, u32); 7] = [
    ("whole screen", 0, 0, WIDTH as u32, HEIGHT as u32),
    ("all rows, 200 wide", 20, 0, 200, HEIGHT as u32),
    ("all rows, 120 wide", 60, 0, 120, HEIGHT as u32),
    ("all rows, 32 wide", 104, 0, 32, HEIGHT as u32),
    ("all rows, 16 wide", 112, 0, 16, HEIGHT as u32),
    ("20 rows, full width", 0, 60, WIDTH as u32, 20),
    ("a 20x20 sprite", 110, 60, 20, 20),
];

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);

    let (mut display, _backlight) =
        cardputer_adv::frame_display(cardputer_adv::display_pins!(p), p.DMA_CH1).expect("display");
    let frame = FRAME.take();
    display.flush(frame).await.expect("flush");

    // Let the log catch up with the start-up.
    Timer::after_millis(500).await;
    info!("flush timing, {FLUSHES} flushes a case:");
    for (name, x, y, width, height) in CASES {
        let area = Rectangle::new(Point::new(x, y), Size::new(width, height));
        let micros = time(&mut display, frame, area).await;
        let bytes = u64::from(width * height * 2);
        info!(
            "{name:>20}: {micros:>6} us, {:>5} kB/s",
            bytes * 1000 / micros.max(1)
        );
    }
    info!("done");
    loop {
        Timer::after_secs(60).await;
    }
}

/// The mean time, in µs, of a flush with `area` redrawn in a new colour.
async fn time(display: &mut FrameDisplay, frame: &mut Frame, area: Rectangle) -> u64 {
    let mut total = 0;
    for flush in 0..FLUSHES {
        // A colour different from the last, so every row in it is sent.
        let colour = if flush % 2 == 0 {
            Rgb565::RED
        } else {
            Rgb565::BLUE
        };
        let Ok(()) = frame.fill_solid(&area, colour);
        let start = Instant::now();
        display.flush(frame).await.expect("flush");
        total += start.elapsed().as_micros();
    }
    total / u64::from(FLUSHES)
}
