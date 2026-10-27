//! Tries the parts on a Cardputer ADV, logging what each does: the RGB LED
//! cycles through colours, G0 sends an NEC code over IR (a phone camera shows
//! the IR LED), the motion sensor and the microphone's level are logged, and
//! the microSD card's size, if there's one.
//!
//! `just hardware`, or `cargo run --release --example hardware --features sd-card`

#![no_std]
#![no_main]
#![expect(
    clippy::future_not_send,
    reason = "esp-hal's async drivers aren't Send; each task runs on the executor that made it"
)]

use cardputer_adv::{AudioPins, Button, Imu, IrTransmitter, RgbLed, embedded_sdmmc::BlockDevice};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::{pixelcolor::Rgb888, prelude::*};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, interrupt::Priority, timer::timg::TimerGroup};
use esp_rtos::embassy::InterruptExecutor;
use log::{info, warn};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

/// The audio task's executor: above the main one, so the main tasks (and
/// their logging) never starve the streams.
static AUDIO_EXECUTOR: StaticCell<InterruptExecutor<1>> = StaticCell::new();

/// What the microphone heard over the last half second, for the main task to
/// log: logging in the audio task would hold the streams up.
static HEARD: Signal<CriticalSectionRawMutex, Heard> = Signal::new();

/// Half a second of the microphone.
struct Heard {
    /// The mean sample: any DC offset.
    mean: i64,
    /// The level around the mean (RMS).
    level: u64,
    peak: u16,
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);
    info!("hardware test");

    // The backlight's switch also powers the RGB LED.
    let (_display, _backlight) =
        cardputer_adv::display(cardputer_adv::display_pins!(p)).expect("display");
    let bus = cardputer_adv::i2c(cardputer_adv::i2c_pins!(p)).expect("I2C bus");

    let card = cardputer_adv::sd_card(cardputer_adv::sd_card_pins!(p)).expect("microSD bus");
    match card.num_blocks() {
        Ok(blocks) => info!("microSD: {} MB", u64::from(blocks.0) * 512 / 1_000_000),
        Err(error) => warn!("microSD: {error:?}"),
    }

    let rmt = cardputer_adv::rmt(p.RMT).expect("RMT");
    let led = cardputer_adv::rgb_led(rmt.channel0, p.GPIO21).expect("RGB LED");
    let ir = cardputer_adv::ir_transmitter(rmt.channel1, p.GPIO44).expect("IR");
    let button = cardputer_adv::button(p.GPIO0);

    let mut imu = cardputer_adv::imu(bus).expect("motion sensor");
    imu.set_gyroscope(true).expect("gyroscope");

    // The streams are made in their task (they aren't Send); the codec
    // clocks itself from them, so it's set up once they run.
    AUDIO_EXECUTOR
        .init(InterruptExecutor::new(p.FROM_CPU_INTR1))
        .start(Priority::Priority2)
        .spawn(listen(cardputer_adv::audio_pins!(p)).expect("audio task"));
    Timer::after(Duration::from_millis(20)).await;
    let mut codec = cardputer_adv::codec(bus).expect("codec");
    codec.set_microphone_gain(24).expect("microphone gain");
    codec.set_microphone(true).expect("microphone");
    info!("{codec:?}");
    spawner.spawn(colours(led).expect("colours task"));
    spawner.spawn(remote(button, ir).expect("remote task"));
    report(imu).await
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
    for press in 1.. {
        button.wait_for_press().await;
        info!("G0: press {press}, sending NEC 0x00/0x45");
        if let Err(error) = ir.send_nec(0x00, 0x45).await {
            warn!("IR: {error}");
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

/// Logs the motion sensor and the microphone, each second.
async fn report(mut imu: Imu) -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        match (imu.acceleration(), imu.rotation()) {
            (Ok(mg), Ok(mdps)) => info!("motion: {mg:?} mg, {mdps:?} m°/s"),
            (Err(error), _) | (_, Err(error)) => warn!("motion: {error}"),
        }
        if let Some(heard) = HEARD.try_take() {
            let Heard { mean, level, peak } = heard;
            info!("microphone: level {level}, peak {peak}, mean {mean}");
        }
        ticker.next().await;
    }
}
