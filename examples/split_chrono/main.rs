#![no_std]
#![no_main]

// Test des limites de l'ecran : chrono MM:SS ou, a chaque seconde, les
// chiffres qui changent affichent d'abord la moitie HAUTE du nouveau chiffre
// (le bas montre encore l'ancien chiffre), puis BOTTOM_DELAY_MS plus tard la
// moitie BASSE du nouveau chiffre (chiffre complet).
//
// Un rafraichissement partiel bloque ~0,42 s (constructeur) : le bas ne peut
// pas apparaitre avant la fin de celui du haut. Le programme MESURE et affiche
// sur la console, pour chaque seconde (temps depuis le changement de seconde) :
//   haut visible a +X ms | bas demande a +Y ms | bas visible a +Z ms
// et la duree de chaque affichage (dessin + envoi SPI + rafraichissement).

mod draw;

use draw::Face;
use esp32_quick::FrameBuffer;
use esp32_quick::clock::uptime_ms;
use esp32_quick::display::Display;
use esp32_quick::{board, board_display};
use esp_hal::delay::Delay;
use esp_hal::main;
use heapless::String as HeapString;

esp_bootloader_esp_idf::esp_app_desc!();

/// Delai souhaite entre le debut de l'affichage du haut et celui du bas.
const BOTTOM_DELAY_MS: u64 = 100;
const CENTER_Y: i32 = 240;

type Text = HeapString<16>;

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();
    let mut display = Display::new(board_display!(peripherals), board::frame_buffer());

    esp_println::println!("== Chrono haut puis bas : test des limites de l'ecran ==");
    esp_println::println!("   bas demande {} ms apres le haut", BOTTOM_DELAY_MS);

    // Premier affichage (rafraichissement complet), puis depart du chrono.
    display.draw(|canvas| render(canvas, "00:00", "00:00", false));
    let start = uptime_ms();
    let mut shown = format_min_sec(0);

    loop {
        let elapsed = uptime_ms() - start;
        let text = format_min_sec(elapsed);
        if text == shown {
            delay.delay_millis(5);
            continue;
        }

        // Instant theorique du changement de seconde (reference des mesures).
        let second_start = start + elapsed / 1000 * 1000;

        // Image 1 : haut du nouveau chiffre, bas de l'ancien.
        let t0 = uptime_ms();
        display.draw_live(|canvas| render(canvas, &text, &shown, true));
        let top_visible = uptime_ms();

        // Image 2 : moitie basse, BOTTOM_DELAY_MS apres le debut de l'image 1.
        // Si l'image 1 a pris plus longtemps (c'est le cas : ~0,45 s), le bas
        // est demande tout de suite apres : c'est la limite de l'ecran.
        let wanted = t0 + BOTTOM_DELAY_MS;
        while uptime_ms() < wanted {
            delay.delay_millis(1);
        }
        let bottom_requested = uptime_ms();
        display.draw_live(|canvas| render(canvas, &text, &text, false));
        let bottom_visible = uptime_ms();

        esp_println::println!(
            "{} | debut a +{} ms | haut visible a +{} ms (affichage {} ms) | bas demande a +{} ms | bas visible a +{} ms (affichage {} ms)",
            text,
            t0 - second_start,
            top_visible - second_start,
            top_visible - t0,
            bottom_requested - second_start,
            bottom_visible - second_start,
            bottom_visible - bottom_requested,
        );
        shown = text;
    }
}

/// `split` : les chiffres de `text` differents de `previous` affichent le
/// haut du nouveau chiffre et le bas de l'ancien ; les autres sont complets.
fn render(canvas: &mut FrameBuffer, text: &str, previous: &str, split: bool) {
    canvas.text_centered("Chrono - le haut, puis le bas", 50);
    let previous = previous.as_bytes();
    draw::draw_row(canvas, text, CENTER_Y, |index, digit| {
        let old = previous.get(index).and_then(|&c| (c as char).to_digit(10)).map(|d| d as u8);
        match old {
            Some(old) if split && old != digit => Face::Mixed { top: digit, bottom: old },
            _ => Face::Full(digit),
        }
    });
    canvas.text_centered("Mesures detaillees sur la console", 450);
}

/// "MM:SS", arrondi a la seconde.
fn format_min_sec(ms: u64) -> Text {
    let secs = ms / 1000;
    let mut text = Text::new();
    let _ = core::fmt::write(&mut text, format_args!("{:02}:{:02}", secs / 60, secs % 60));
    text
}
