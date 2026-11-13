#![no_std]
#![no_main]

// Chrono "adaptatif" : a chaque nouvelle seconde, s'il reste assez de temps
// avant la seconde suivante pour une belle transition (2 images : haut du
// nouveau chiffre / bas de l'ancien, puis chiffre complet), on la fait ;
// sinon on affiche directement le chiffre (1 image).
//
// La duree d'un affichage est MESUREE en continu (moyenne glissante) : le
// programme s'adapte tout seul a la vitesse reelle de l'ecran (qui ralentit
// par exemple au froid) et se recale apres un retard.
//
// Dessins : ceux de l'exemple split_chrono (moities haute et basse separees).

#[path = "../split_chrono/draw.rs"]
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

/// Estimation de depart de la duree d'un affichage (avant les premieres
/// mesures) : volontairement prudente.
const INITIAL_FRAME_MS: u64 = 550;
/// Marge de securite ajoutee au temps necessaire pour une transition.
const MARGIN_MS: u64 = 60;
const CENTER_Y: i32 = 240;

type Text = HeapString<16>;

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();
    let mut display = Display::new(board_display!(peripherals), board::frame_buffer());

    esp_println::println!("== Chrono adaptatif : transition seulement si le temps le permet ==");

    // Premier affichage (rafraichissement complet), puis depart du chrono.
    display.draw(|canvas| render(canvas, "00:00", "00:00", false, ""));
    let start = uptime_ms();
    let mut shown = format_min_sec(0);
    // Duree estimee d'un affichage partiel (moyenne glissante des mesures).
    let mut frame_ms = INITIAL_FRAME_MS;
    let (mut animated, mut direct) = (0u32, 0u32);

    loop {
        let now = uptime_ms();
        let elapsed = now - start;
        let text = format_min_sec(elapsed);
        if text == shown {
            delay.delay_millis(5);
            continue;
        }

        // Temps restant avant la seconde suivante.
        let remaining = 1000 - elapsed % 1000;
        let needed = 2 * frame_ms + MARGIN_MS;
        let animate = remaining >= needed;

        if animate {
            // Belle transition : haut du nouveau / bas de l'ancien, puis complet.
            let t = uptime_ms();
            display.draw_live(|canvas| render(canvas, &text, &shown, true, "transition"));
            frame_ms = average(frame_ms, uptime_ms() - t);
            animated += 1;
        } else {
            direct += 1;
        }
        let t = uptime_ms();
        display.draw_live(|canvas| render(canvas, &text, &text, false, if animate { "transition" } else { "direct" }));
        frame_ms = average(frame_ms, uptime_ms() - t);

        esp_println::println!(
            "{} | reste {} ms, transition demande {} ms -> {} | affichage estime {} ms | total : {} transitions, {} directs",
            text,
            remaining,
            needed,
            if animate { "TRANSITION" } else { "direct" },
            frame_ms,
            animated,
            direct,
        );
        shown = text;
    }
}

/// Moyenne glissante : 3/4 de l'ancienne estimation + 1/4 de la mesure.
fn average(estimate: u64, measured: u64) -> u64 {
    (3 * estimate + measured) / 4
}

/// `split` : les chiffres de `text` differents de `previous` affichent le
/// haut du nouveau chiffre et le bas de l'ancien ; les autres sont complets.
fn render(canvas: &mut FrameBuffer, text: &str, previous: &str, split: bool, mode: &str) {
    canvas.text_centered("Chrono adaptatif", 50);
    let previous = previous.as_bytes();
    draw::draw_row(canvas, text, CENTER_Y, |index, digit| {
        let old = previous.get(index).and_then(|&c| (c as char).to_digit(10)).map(|d| d as u8);
        match old {
            Some(old) if split && old != digit => Face::Mixed { top: digit, bottom: old },
            _ => Face::Full(digit),
        }
    });
    canvas.text_centered(mode, 420);
}

/// "MM:SS", arrondi a la seconde.
fn format_min_sec(ms: u64) -> Text {
    let secs = ms / 1000;
    let mut text = Text::new();
    let _ = core::fmt::write(&mut text, format_args!("{:02}:{:02}", secs / 60, secs % 60));
    text
}
