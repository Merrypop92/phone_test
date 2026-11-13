#![no_std]
#![no_main]

// Reproduction du chrono a palettes, sans le reste de l'application, pour
// trouver d'ou venaient les debordements rectangulaires (palettes qui
// "depassent" sur les cotes en haut et en bas) vus avec l'envoi des seules
// lignes modifiees.
//
// La meme sequence est jouee deux fois :
//   A. envoi des SEULES lignes modifiees (meme code que l'etape 2 :
//      comparaison avec une copie de l'image affichee, fenetre pleine largeur
//      sur les lignes qui different, RAM 0x24 seulement) ;
//   B. envoi de TOUT l'ecran en partiel (methode actuelle de l'application).
//
// Sequence : rafraichissement complet sur "00:00.0" (chrono arrete, 7
// elements), puis "00:00" (en marche, 5 elements : la disposition change),
// puis 00:01, 00:02... avec l'animation de bascule (image 1 : seuls les hauts
// changent ; image 2 : les bas suivent).

use esp32_quick::display::flap;
use esp32_quick::{BUF_SIZE, Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, board, board_display, clear_full_buffer};
use esp_hal::delay::Delay;
use esp_hal::main;

esp_bootloader_esp_idf::esp_app_desc!();

const ROW_BYTES: usize = (PX_WIDTH / 8) as usize;
const FLAP_CENTER_Y: i32 = 230;
/// Valeurs successives du chrono en marche.
const SEQUENCE: [&str; 7] = ["00:00", "00:01", "00:02", "00:03", "00:04", "00:05", "00:06"];

/// Copie de l'image affichee (methode A). Utilisee seulement dans `main`.
static mut DISPLAYED: [u8; BUF_SIZE] = [0xFF; BUF_SIZE];

#[derive(Clone, Copy, PartialEq)]
enum Method {
    /// Seules les lignes modifiees.
    Band,
    /// Tout l'ecran.
    FullScreen,
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut delay = Delay::new();
    let mut epd = board_display!(peripherals);
    let buf = board::frame_buffer();
    // SAFETY : seule reference a DISPLAYED, prise une fois ici.
    let displayed: &mut [u8] = unsafe { &mut *core::ptr::addr_of_mut!(DISPLAYED) };

    esp_println::println!("== Test des palettes : envoi par bande (A) / plein ecran (B) ==");

    for method in [Method::Band, Method::FullScreen] {
        let title = match method {
            Method::Band => "A : seules les lignes modifiees",
            Method::FullScreen => "B : tout l'ecran (methode actuelle)",
        };
        esp_println::println!("--- {} ---", title);

        // Rafraichissement complet sur le chrono arrete.
        render(buf, title, "00:00.0", "00:00.0");
        epd.hw_init(&mut delay);
        epd.set_ram_value_basemap(buf);
        displayed.copy_from_slice(buf);
        esp_println::println!("    complet : 00:00.0");
        delay.delay_millis(2000);

        // Chrono en marche : changement de disposition, puis une valeur par
        // seconde, avec l'animation de bascule.
        let mut previous = "00:00.0";
        for text in SEQUENCE {
            // Image 1 : seuls les hauts changent (directe si la disposition change).
            render(buf, title, text, previous);
            show(&mut epd, buf, displayed, method);
            // Image 2 : les bas rejoignent les hauts.
            render(buf, title, text, text);
            show(&mut epd, buf, displayed, method);
            esp_println::println!("    {} -> {}", previous, text);
            previous = text;
            delay.delay_millis(1000);
        }

        esp_println::println!("    Regarde bien les palettes avant la suite (5 s).");
        delay.delay_millis(5000);
    }

    esp_println::println!("== Termine. Les debordements apparaissent-ils en A, en B, ou dans les deux ? ==");
    esp_println::println!("   Et a quel moment : au passage 00:00.0 -> 00:00, ou plus tard ?");
    loop {
        delay.delay_millis(1000);
    }
}

/// Dessine l'ecran : titre + rangee de palettes (haut = `top`, bas = `bottom`).
fn render(buf: &mut [u8], title: &str, top: &str, bottom: &str) {
    clear_full_buffer(buf);
    let mut canvas = FrameBuffer { buf, width: PX_WIDTH, height: PX_HEIGHT };
    canvas.text_centered(title, 40);
    flap::draw_row_halves(&mut canvas, top, bottom, FLAP_CENTER_Y);
}

/// Envoie l'image en rafraichissement partiel, selon la methode.
fn show(epd: &mut Epd, buf: &[u8], displayed: &mut [u8], method: Method) {
    match method {
        Method::FullScreen => {
            epd.dis_part_ram(0, 0, buf, PX_WIDTH, PX_HEIGHT);
            epd.part_update();
        }
        Method::Band => {
            // Meme logique que l'etape 2.
            let changed = |y: &usize| buf[y * ROW_BYTES..(y + 1) * ROW_BYTES] != displayed[y * ROW_BYTES..(y + 1) * ROW_BYTES];
            let Some(first) = (0..PX_HEIGHT as usize).find(changed) else {
                return; // rien n'a change
            };
            let last = (0..PX_HEIGHT as usize).rev().find(changed).unwrap_or(first);
            let rows = &buf[first * ROW_BYTES..(last + 1) * ROW_BYTES];
            epd.dis_part_ram(0, first as u32, rows, PX_WIDTH, (last - first + 1) as u32);
            epd.part_update();
            esp_println::println!("      bande envoyee : lignes {} a {}", first, last);
        }
    }
    displayed.copy_from_slice(buf);
}
