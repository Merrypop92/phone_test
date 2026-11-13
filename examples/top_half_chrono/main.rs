#![no_std]
#![no_main]

// Chrono "moitie haute" : comme un chrono MM:SS, mais chaque carte n'affiche
// que le DESSIN de la partie superieure du chiffre (voir draw.rs, tableau
// TOP_HALVES, modifiable a la main). Sans transition : une seule image par
// seconde (1 rafraichissement partiel). Demarre tout seul au lancement.

mod draw;

use esp32_quick::FrameBuffer;
use esp32_quick::clock::uptime_ms;
use esp32_quick::display::Display;
use esp32_quick::{board, board_display};
use esp_hal::delay::Delay;
use esp_hal::main;
use heapless::String as HeapString;

esp_bootloader_esp_idf::esp_app_desc!();

/// Intervalle de verification du temps.
const TICK_MS: u32 = 20;
const CENTER_Y: i32 = 240;

type Text = HeapString<16>;

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();
    // Premier affichage : rafraichissement complet ; ensuite partiels (plein
    // ecran), avec un complet de temps en temps (Display/Epd).
    let mut display = Display::new(board_display!(peripherals), board::frame_buffer());

    esp_println::println!("== Chrono moitie haute : demarre ==");
    let start = uptime_ms();
    let mut shown = format_min_sec(0);
    display.draw(|canvas| render(canvas, &shown));

    loop {
        let text = format_min_sec(uptime_ms() - start);
        if text != shown {
            display.draw_live(|canvas| render(canvas, &text));
            esp_println::println!("{}", text);
            shown = text;
        }
        delay.delay_millis(TICK_MS);
    }
}

fn render(canvas: &mut FrameBuffer, text: &Text) {
    canvas.text_centered("Chrono - moitie haute des chiffres", 50);
    draw::draw_row(canvas, text.as_str(), CENTER_Y);
    canvas.text_centered("Dessins modifiables : TOP_HALVES dans draw.rs", 450);
}

/// "MM:SS", arrondi a la seconde.
fn format_min_sec(ms: u64) -> Text {
    let secs = ms / 1000;
    let mut text = Text::new();
    let _ = core::fmt::write(&mut text, format_args!("{:02}:{:02}", secs / 60, secs % 60));
    text
}
