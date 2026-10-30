//! The smallest Cardputer ADV app: what you type shows on the screen.
//! Backspace takes a character off, Enter starts the line again, and a held
//! key repeats.
//!
//! `cargo run --release --example hello`

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

use cardputer_adv::{Repeat, WIDTH, keys::KeyInput};
use embassy_executor::Spawner;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};

esp_bootloader_esp_idf::esp_app_desc!();

/// The characters that fit across the screen, 10 pixels each.
const COLUMNS: usize = WIDTH as usize / 10;

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);

    let (mut display, _backlight) =
        cardputer_adv::display(cardputer_adv::display_pins!(p)).expect("display");
    let mut keyboard = cardputer_adv::keyboard(cardputer_adv::keyboard_pins!(p))
        .await
        .expect("keyboard");
    keyboard.set_repeat(Some(Repeat::default()));

    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let blank = PrimitiveStyle::with_fill(Rgb565::BLACK);
    let line_area = Rectangle::new(Point::new(0, 60), Size::new(WIDTH.into(), 20));
    display.clear(Rgb565::BLACK).expect("LCD");
    Text::with_baseline("Type something:", Point::new(0, 20), style, Baseline::Top)
        .draw(&mut display)
        .expect("LCD");

    let mut line = [0; COLUMNS];
    let mut len = 0;
    loop {
        for key in keyboard.wait_for_inputs().await.expect("keyboard") {
            match key {
                KeyInput::Char(c) if c.is_ascii() && len < COLUMNS => {
                    c.encode_utf8(&mut line[len..]);
                    len += 1;
                }
                KeyInput::Backspace => len = len.saturating_sub(1),
                KeyInput::Enter => len = 0,
                _ => {}
            }
        }
        let text = str::from_utf8(&line[..len]).unwrap_or_default();
        line_area
            .into_styled(blank)
            .draw(&mut display)
            .expect("LCD");
        Text::with_baseline(text, line_area.top_left, style, Baseline::Top)
            .draw(&mut display)
            .expect("LCD");
    }
}
