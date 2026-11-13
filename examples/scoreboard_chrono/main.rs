#![no_std]
#![no_main]

// Chrono facon tableau de score a l'ancienne : chaque chiffre est une carte
// entiere accrochee en haut a une barre (voir draw.rs). Demarre tout seul au
// lancement, compte en MM:SS.
//
// Bascule animee (2 rafraichissements partiels : carte en train de se
// soulever, puis carte suivante) pour les secondes paires ; changement direct
// (1 rafraichissement) pour les impaires : ~1,4 s de travail toutes les 2 s.

mod draw;

use esp32_quick::FrameBuffer;
use esp32_quick::clock::uptime_ms;
use esp32_quick::display::Display;
use esp32_quick::{board, board_display};
use esp_hal::delay::Delay;
use esp_hal::main;
use heapless::String as HeapString;

esp_bootloader_esp_idf::esp_app_desc!();

/// Animation de bascule.
const ANIMATE: bool = true;
/// Animer seulement les secondes paires (sinon : toutes, ~2 images par seconde,
/// a la limite de l'ecran).
const ANIMATE_EVEN_ONLY: bool = true;
/// Intervalle de verification du temps.
const TICK_MS: u32 = 20;
const CENTER_Y: i32 = 250;

type Text = HeapString<16>;

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();
    // Le premier affichage est un rafraichissement complet, puis partiels
    // (plein ecran), avec un complet de temps en temps (Display/Epd).
    let mut display = Display::new(board_display!(peripherals), board::frame_buffer());

    esp_println::println!("== Chrono tableau de score : demarre ==");
    let start = uptime_ms();
    let mut shown = format_min_sec(0);
    display.draw(|canvas| render(canvas, &shown, None));

    loop {
        let text = format_min_sec(uptime_ms() - start);
        if text != shown {
            let animate = ANIMATE && (!ANIMATE_EVEN_ONLY || ends_with_even_digit(&text));
            if animate {
                // Image 1 : l'ancienne carte se souleve, la nouvelle apparait dessous.
                display.draw_live(|canvas| render(canvas, &text, Some(&shown)));
            }
            // Image finale : la nouvelle carte pend seule.
            display.draw_live(|canvas| render(canvas, &text, None));
            esp_println::println!("{}{}", text, if animate { " (anime)" } else { "" });
            shown = text;
        }
        delay.delay_millis(TICK_MS);
    }
}

fn render(canvas: &mut FrameBuffer, text: &Text, lifting: Option<&Text>) {
    canvas.text_centered("Chrono - tableau de score", 50);
    draw::draw_scoreboard(canvas, text.as_str(), lifting.map(|t| t.as_str()), CENTER_Y);
    canvas.text_centered("Secondes paires : bascule animee | impaires : directe", 450);
}

/// "MM:SS", arrondi a la seconde.
fn format_min_sec(ms: u64) -> Text {
    let secs = ms / 1000;
    let mut text = Text::new();
    let _ = core::fmt::write(&mut text, format_args!("{:02}:{:02}", secs / 60, secs % 60));
    text
}

fn ends_with_even_digit(text: &str) -> bool {
    text.chars().last().and_then(|c| c.to_digit(10)).is_some_and(|d| d % 2 == 0)
}
