//! Tries the parts on a Cardputer ADV. The screen lists what to do, and
//! each step turns green once it's seen to work:
//!
//! 1. Lie it flat and still while it starts: the gyroscope calibrates.
//! 2. Press G0 three times: each press sends an NEC code over IR (a phone
//!    camera shows the IR LED), and moves the motion sensor on to its next
//!    power mode (step 4's line names it; low-power reads noisier).
//! 3. Clap near it: the microphone.
//! 4. Tilt it on its side: the motion sensor. The gyroscope's line shows
//!    how fast it's turning, so it reads about 0 whenever it's still.
//! 5. Watch the RGB LED cycle red, green, blue and off. Nothing can see it,
//!    so this line stays white.
//! 6. Type a capital (Shift) and an arrow (Fn + `;` `,` `.` `/`): the
//!    keyboard. Its line shows the last key typed, and the log whether
//!    Shift and Fn are held after each.
//!
//! The microSD card's size shows too, if there's one, and everything is
//! logged over USB as well.
//!
//! `just hardware`, or `cargo run --release --example hardware --features sd-card`

#![no_std]
#![no_main]
#![expect(
    clippy::future_not_send,
    reason = "esp-hal's async drivers aren't Send; each task runs on the executor that made it"
)]

use core::{
    fmt::{self, Write as _},
    str,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use cardputer_adv::{
    Acceleration, AudioPins, Button, Decibels, Display, Imu, ImuMode, IrTransmitter, Keyboard,
    NecCode, Pulse, RgbLed, Rotation,
    embedded_sdmmc::BlockDevice,
    keys::{KeyInput, Modifier},
};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::{
    mono_font::{MonoTextStyle, MonoTextStyleBuilder, ascii::FONT_6X10},
    pixelcolor::{Rgb565, Rgb888},
    prelude::*,
    text::{Baseline, Text},
};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, interrupt::Priority, timer::timg::TimerGroup};
use esp_rtos::embassy::InterruptExecutor;
use log::{info, warn};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

/// The audio task's executor: above the main one, so the main tasks (and
/// their logging) never starve the streams.
static AUDIO_EXECUTOR: StaticCell<InterruptExecutor<1>> = StaticCell::new();

/// What the microphone heard over the last half second, for the main task
/// to show: drawing or logging in the audio task would hold the streams up.
static HEARD: Signal<CriticalSectionRawMutex, Heard> = Signal::new();

/// G0 presses so far.
static PRESSES: AtomicU32 = AtomicU32::new(0);

/// The last key typed, for the main task to show.
static TYPED: Signal<CriticalSectionRawMutex, KeyInput> = Signal::new();
/// Whether a capital (Shift) and an arrow (Fn) have been typed.
static SHIFTED: AtomicBool = AtomicBool::new(false);
static FN_LAYER: AtomicBool = AtomicBool::new(false);

/// Half a second of the microphone.
struct Heard {
    /// The mean sample: any DC offset.
    mean: i64,
    /// The level around the mean (RMS).
    level: u64,
    peak: u16,
}

/// A clap's peak is in the tens of thousands; a quiet room's, a few dozen.
const CLAP_PEAK: u16 = 8000;
/// On its side, gravity is mostly along x or y (milli-g).
const TILTED_MG: i32 = 700;
/// The motion sensor's modes, a G0 press each.
const MODES: [ImuMode; 3] = [ImuMode::Performance, ImuMode::Normal, ImuMode::LowPower];

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);
    info!("hardware test");

    // The backlight's switch also powers the RGB LED.
    let (display, _backlight) =
        cardputer_adv::display(cardputer_adv::display_pins!(p)).expect("display");
    let mut screen = Screen::new(display);
    screen.show(0, false, format_args!("HARDWARE TEST - keep it still..."));
    let bus = cardputer_adv::i2c(cardputer_adv::i2c_pins!(p)).expect("I2C bus");

    let mut imu = cardputer_adv::imu(bus).await.expect("motion sensor");
    imu.calibrate_gyroscope().await.expect("gyroscope");
    info!("{imu:?}");
    let keyboard = cardputer_adv::keyboard_on(bus, p.GPIO11)
        .await
        .expect("keyboard");

    let card = cardputer_adv::sd_card(cardputer_adv::sd_card_pins!(p)).expect("microSD bus");
    match card.num_blocks() {
        Ok(blocks) => {
            let megabytes = u64::from(blocks.0) * 512 / 1_000_000;
            info!("microSD: {megabytes} MB");
            screen.show(9, true, format_args!("microSD: {megabytes} MB"));
        }
        Err(error) => {
            warn!("microSD: {error:?}");
            screen.show(9, false, format_args!("microSD: none ({error:?})"));
        }
    }

    let rmt = cardputer_adv::rmt(p.RMT).expect("RMT");
    let led = cardputer_adv::rgb_led(rmt.channel0, p.GPIO21).expect("RGB LED");
    let ir = cardputer_adv::ir_transmitter(rmt.channel1, p.GPIO44).expect("IR");
    let button = cardputer_adv::button(p.GPIO0);

    // The streams are made in their task (they aren't Send); the codec
    // clocks itself from them, so it's set up once they run.
    AUDIO_EXECUTOR
        .init(InterruptExecutor::new(p.FROM_CPU_INTR1))
        .start(Priority::Priority2)
        .spawn(listen(cardputer_adv::audio_pins!(p)).expect("audio task"));
    Timer::after(Duration::from_millis(20)).await;
    let mut codec = cardputer_adv::codec(bus).await.expect("codec");
    codec
        .set_microphone_gain(Decibels(24))
        .await
        .expect("microphone gain");
    codec.set_microphone(true).await.expect("microphone");
    info!("{codec:?}");
    spawner.spawn(colours(led).expect("colours task"));
    spawner.spawn(remote(button, ir).expect("remote task"));
    spawner.spawn(typing(keyboard).expect("keyboard task"));
    report(screen, imu).await
}

#[embassy_executor::task]
async fn colours(mut led: RgbLed) {
    let colours = [Rgb888::RED, Rgb888::GREEN, Rgb888::BLUE, Rgb888::BLACK];
    let mut ticker = Ticker::every(Duration::from_millis(500));
    for colour in colours.into_iter().cycle() {
        if let Err(error) = led.set_color(colour).await {
            warn!("LED: {error}");
        }
        ticker.next().await;
    }
}

#[embassy_executor::task]
async fn remote(mut button: Button, mut ir: IrTransmitter) {
    loop {
        button.wait_for_press().await;
        let press = PRESSES.fetch_add(1, Ordering::Relaxed) + 1;
        info!("G0: press {press}, sending NEC 0x00/0x45");
        let code = NecCode {
            address: 0x00,
            command: 0x45,
        };
        if let Err(error) = ir.send_nec(code).await {
            warn!("IR: {error}");
        }
        // The same code's repeat, as raw pulses: what send_raw sends for
        // other protocols.
        Timer::after(Duration::from_millis(40)).await;
        match ir
            .send_raw(&[Pulse::new(9000, 2250), Pulse::new(562, 0)])
            .await
        {
            Ok(()) => info!("IR: sent a raw code"),
            Err(error) => warn!("IR raw: {error}"),
        }
    }
}

/// Logs each key typed, and passes it on to be shown.
#[embassy_executor::task]
async fn typing(mut keyboard: Keyboard) {
    loop {
        match keyboard.wait_for_inputs().await {
            Ok(inputs) => {
                for input in inputs {
                    info!("key: {input:?}");
                    match input {
                        KeyInput::Char(c) if c.is_ascii_uppercase() => {
                            SHIFTED.store(true, Ordering::Relaxed);
                        }
                        KeyInput::Arrow(_) => FN_LAYER.store(true, Ordering::Relaxed),
                        _ => {}
                    }
                    TYPED.signal(input);
                }
                let [shift, fn_held] = [Modifier::Shift, Modifier::Fn]
                    .map(|modifier| keyboard.is_held(KeyInput::Modifier(modifier)));
                info!("held: Shift {shift}, Fn {fn_held}");
            }
            Err(error) => {
                warn!("keyboard: {error}");
                Timer::after(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Plays silence (the microphone runs on the speaker's clocks) and measures
/// what the microphone hears, each half second.
#[embassy_executor::task]
async fn listen(pins: AudioPins) {
    let mut audio = cardputer_adv::audio(pins).expect("audio");
    let (mut sum, mut squares, mut peak, mut count) = (0i64, 0u64, 0u16, 0u32);
    let take = |samples: &[i16]| {
        for &sample in samples {
            sum += i64::from(sample);
            squares += u64::from(sample.unsigned_abs()).pow(2);
            peak = peak.max(sample.unsigned_abs());
        }
        count += u32::try_from(samples.len()).unwrap_or(u32::MAX);
        if count >= cardputer_adv::SAMPLE_RATE / 2 {
            let mean = sum / i64::from(count);
            let level = (squares / u64::from(count))
                .saturating_sub(mean.unsigned_abs().pow(2))
                .isqrt();
            HEARD.signal(Heard { mean, level, peak });
            (sum, squares, peak, count) = (0, 0, 0, 0);
        }
    };
    let Err(error) = audio.run(|samples| samples.fill(0), take).await;
    warn!("audio stopped: {error}");
}

/// Shows the steps and what each part reads, and logs it, twice a second.
async fn report(mut screen: Screen, mut imu: Imu) -> ! {
    screen.show(0, false, format_args!("HARDWARE TEST - do each step:"));
    screen.show(1, true, format_args!("1. Lie still: gyro calibrated"));
    screen.show(
        8,
        false,
        format_args!("5. By eye: LED red, green, blue, off"),
    );
    let (mut loudest, mut tilted, mut typed, mut mode_presses) = (0, false, None, 0);
    let mut ticker = Ticker::every(Duration::from_millis(500));
    loop {
        let presses = PRESSES.load(Ordering::Relaxed);
        screen.show(
            2,
            presses >= 3,
            format_args!("2. Press G0 3 times: {presses}"),
        );
        if presses != mode_presses {
            mode_presses = presses;
            let mode = MODES[presses as usize % MODES.len()];
            match imu.set_mode(mode).await {
                Ok(()) => info!("motion sensor: {mode:?}"),
                Err(error) => warn!("motion sensor mode: {error}"),
            }
        }

        if let Some(Heard { mean, level, peak }) = HEARD.try_take() {
            info!("microphone: level {level}, peak {peak}, mean {mean}");
            loudest = loudest.max(peak);
        }
        screen.show(
            3,
            loudest >= CLAP_PEAK,
            format_args!("3. Clap near it: loudest {loudest}"),
        );

        match (imu.acceleration().await, imu.rotation().await) {
            (Ok(acceleration @ Acceleration { x, y, z }), Ok(rotation)) => {
                info!("motion: {acceleration:?} mg, {rotation:?} m°/s");
                tilted |= x.abs() >= TILTED_MG || y.abs() >= TILTED_MG;
                let mode = imu.mode();
                screen.show(4, tilted, format_args!("4. Tilt it on its side ({mode:?})"));
                screen.show(5, false, format_args!("   accel {x} {y} {z} mg"));
                let Rotation { x, y, z } = rotation;
                let [rx, ry, rz] = [x, y, z].map(|m| m / 1000);
                screen.show(6, false, format_args!("   turning {rx} {ry} {rz} deg/s"));
            }
            (Err(error), _) | (_, Err(error)) => warn!("motion: {error}"),
        }
        typed = TYPED.try_take().or(typed);
        let both_layers = SHIFTED.load(Ordering::Relaxed) && FN_LAYER.load(Ordering::Relaxed);
        match typed {
            Some(key) => screen.show(10, both_layers, format_args!("6. Type A, Fn+;: {key:?}")),
            None => screen.show(10, false, format_args!("6. Type A (Shift), Fn+; (up)")),
        }
        if presses >= 3 && loudest >= CLAP_PEAK && tilted && both_layers {
            screen.show(11, true, format_args!("All seen. Unplug when done."));
        }
        ticker.next().await;
    }
}

/// The LCD as rows of text.
struct Screen {
    display: Display,
}

impl Screen {
    /// Characters a row: the screen's width in 6-pixel glyphs.
    const COLUMNS: usize = 40;
    /// Pixels a row: the 10-pixel font, and a gap.
    const ROW_HEIGHT: i32 = 11;

    fn new(mut display: Display) -> Self {
        display.clear(Rgb565::BLACK).expect("LCD");
        Self { display }
    }

    /// Writes row `row`, green once its step is `done`. The row is padded
    /// with spaces, so what was there before is overwritten.
    fn show(&mut self, row: i32, done: bool, text: fmt::Arguments<'_>) {
        let mut line = Line::new();
        // Longer lines are cut at the screen's edge.
        let _ = line.write_fmt(text);
        line.pad();
        let style: MonoTextStyle<'_, Rgb565> = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(if done { Rgb565::GREEN } else { Rgb565::WHITE })
            .background_color(Rgb565::BLACK)
            .build();
        let top_left = Point::new(0, row * Self::ROW_HEIGHT);
        Text::with_baseline(line.as_str(), top_left, style, Baseline::Top)
            .draw(&mut self.display)
            .expect("LCD");
    }
}

/// One row of [`Screen`]'s text, formatted without an allocator. ASCII
/// only, so cutting it at any byte leaves it valid.
struct Line {
    bytes: [u8; Screen::COLUMNS],
    len: usize,
}

impl Line {
    const fn new() -> Self {
        Self {
            bytes: [0; Screen::COLUMNS],
            len: 0,
        }
    }

    fn pad(&mut self) {
        self.bytes[self.len..].fill(b' ');
        self.len = self.bytes.len();
    }

    fn as_str(&self) -> &str {
        str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

impl fmt::Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let room = self.bytes.len() - self.len;
        let taken = text.len().min(room);
        self.bytes[self.len..self.len + taken].copy_from_slice(&text.as_bytes()[..taken]);
        self.len += taken;
        Ok(())
    }
}
