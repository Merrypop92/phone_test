#![no_std]
#![no_main]

// Test de l'adressage d'une fenetre en rafraichissement partiel.
//
// Question : quand on envoie au controleur une bande de lignes "y = 160 a 199"
// (fenetre RAM 0x44/0x45 + ecriture 0x24), apparait-elle bien a cette hauteur
// sur l'ecran ? En plein ecran on ne peut pas le voir (tout est envoye d'un
// bloc), mais une erreur ici expliquerait que l'envoi des seules lignes
// modifiees ait degrade l'affichage.
//
// Deroulement (une etape toutes les 4 s, suivre la console) :
//   1. Rafraichissement complet : une regle (traits horizontaux tous les 80 px,
//      etiquetes "y=80", "y=160"...), "HAUT"/"BAS", "GAUCHE"/"DROITE".
//   2. Bande A, lignes 160 a 199, envoyee seule (fenetre pleine largeur).
//   3. Bande B, lignes 320 a 359, envoyee seule.
//   4. Petit bloc, fenetre RAM x = 0 a 199, lignes 400 a 439 : apparait-il a
//      gauche ou a droite ? (le FrameBuffer compense un miroir horizontal).
//      Resultat sur la carte : a DROITE (axe horizontal du controleur inverse).
//   5 a 7. Le controleur met-il a jour tout seul sa RAM "ancienne image"
//      (0x26) apres un rafraichissement partiel ? Il ne fait bouger que les
//      pixels qui different entre 0x24 (nouvelle) et 0x26 (ancienne). Si 0x26
//      reste figee sur l'image du dernier rafraichissement complet, un pixel
//      qui REVIENT a sa couleur d'origine n'est jamais redessine :
//      5. carre noir ajoute (partiel plein ecran) ;
//      6. carre efface (partiel plein ecran) : disparait-il ?
//      7. idem avec l'envoi d'une bande seulement.

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp32_quick::{FrameBuffer, PX_HEIGHT, PX_WIDTH, board, board_display, clear_full_buffer};
use esp_hal::delay::Delay;
use esp_hal::main;

esp_bootloader_esp_idf::esp_app_desc!();

const ROW_BYTES: usize = (PX_WIDTH / 8) as usize;
const STEP_PAUSE_MS: u32 = 4000;

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut delay = Delay::new();
    let mut epd = board_display!(peripherals);
    let buf = board::frame_buffer();

    esp_println::println!("== Test de fenetre e-paper (rafraichissement partiel) ==");

    // --- 1. Regle de reference, en rafraichissement complet ---
    esp_println::println!("[1] Regle : traits tous les 80 px, etiquetes y=0 a y=400.");
    clear_full_buffer(buf);
    {
        let mut canvas = FrameBuffer { buf: &mut *buf, width: PX_WIDTH, height: PX_HEIGHT };
        let line = PrimitiveStyle::with_fill(BinaryColor::On);
        for y in (0..PX_HEIGHT as i32).step_by(80) {
            let _ = Rectangle::new(Point::new(0, y), Size::new(PX_WIDTH, 2)).into_styled(line).draw(&mut canvas);
            let mut label: heapless::String<12> = heapless::String::new();
            let _ = core::fmt::write(&mut label, format_args!("y={y}"));
            canvas.text(label.as_str(), 8, y + 22);
        }
        canvas.text_centered("HAUT", 60);
        canvas.text_centered("BAS", 470);
        canvas.text("GAUCHE", 8, 260);
        canvas.text_right("DROITE", 8, 260);
    }
    epd.hw_init(&mut delay);
    epd.set_ram_value_basemap(buf);
    delay.delay_millis(STEP_PAUSE_MS);

    // --- 2. Bande A : lignes 160 a 199 ---
    send_band(&mut epd, buf, 160, 40, "BANDE A : lignes 160 a 199");
    esp_println::println!("[2] Bande A envoyee (lignes 160 a 199).");
    esp_println::println!("    Attendu : juste SOUS le trait y=160.");
    esp_println::println!("    Si elle apparait sous le trait y=240 (vers 280-319) : adressage vertical inverse.");
    delay.delay_millis(STEP_PAUSE_MS);

    // --- 3. Bande B : lignes 320 a 359 ---
    send_band(&mut epd, buf, 320, 40, "BANDE B : lignes 320 a 359");
    esp_println::println!("[3] Bande B envoyee (lignes 320 a 359).");
    esp_println::println!("    Attendu : juste SOUS le trait y=320 (si inverse : vers 120-159, sous y=80).");
    delay.delay_millis(STEP_PAUSE_MS);

    // --- 4. Petit bloc : fenetre RAM x = 0 a 199, lignes 400 a 439 ---
    // Donnees brutes (tout noir), sans passer par le FrameBuffer : on teste
    // l'adressage horizontal du controleur lui-meme.
    let block = [0x00u8; 25 * 40]; // 200 px / 8 = 25 octets par ligne, 40 lignes
    epd.dis_part_ram(0, 400, &block, 200, 40);
    epd.part_update();
    esp_println::println!("[4] Bloc noir envoye : fenetre RAM x = 0 a 199, lignes 400 a 439.");
    esp_println::println!("    Note s'il apparait cote GAUCHE ou cote DROITE de l'ecran (sous le trait y=400).");
    delay.delay_millis(STEP_PAUSE_MS);

    // Le tampon ne contient pas le bloc de l'etape 4 (donnees brutes) : on le
    // redessine pour que le tampon corresponde a l'ecran.
    fill(buf, Rectangle::new(Point::new(600, 400), Size::new(200, 40)), true);

    // --- 5. Carre noir ajoute, partiel plein ecran ---
    let square = Rectangle::new(Point::new(560, 180), Size::new(120, 120));
    fill(buf, square, true);
    epd.dis_part_ram(0, 0, buf, PX_WIDTH, PX_HEIGHT);
    epd.part_update();
    esp_println::println!("[5] Carre noir ajoute a droite (partiel, plein ecran). Il doit apparaitre.");
    delay.delay_millis(STEP_PAUSE_MS);

    // --- 6. Carre efface, partiel plein ecran ---
    fill(buf, square, false);
    epd.dis_part_ram(0, 0, buf, PX_WIDTH, PX_HEIGHT);
    epd.part_update();
    esp_println::println!("[6] Carre efface (partiel, plein ecran).");
    esp_println::println!("    S'il DISPARAIT : le controleur met bien a jour sa RAM 0x26 tout seul.");
    esp_println::println!("    S'il RESTE (meme en partie) : 0x26 reste figee, il faudra l'ecrire nous-memes.");
    delay.delay_millis(STEP_PAUSE_MS);

    // --- 7. Meme chose avec l'envoi d'une bande seulement ---
    fill(buf, square, true);
    send_rows(&mut epd, buf, 180, 120);
    esp_println::println!("[7a] Carre noir ajoute (partiel, bande lignes 180 a 299 seulement).");
    delay.delay_millis(STEP_PAUSE_MS);
    fill(buf, square, false);
    send_rows(&mut epd, buf, 180, 120);
    esp_println::println!("[7b] Carre efface (partiel, bande seulement) : disparait-il ?");

    esp_println::println!("== Termine : dis-moi le resultat des etapes 5, 6 et 7 ==");
    loop {
        delay.delay_millis(1000);
    }
}

/// Remplit `area` du tampon en noir (`black`) ou en blanc.
fn fill(buf: &mut [u8], area: Rectangle, black: bool) {
    let mut canvas = FrameBuffer { buf, width: PX_WIDTH, height: PX_HEIGHT };
    let color = if black { BinaryColor::On } else { BinaryColor::Off };
    let _ = area.into_styled(PrimitiveStyle::with_fill(color)).draw(&mut canvas);
}

/// Envoie seulement les lignes `y..y+height` du tampon (pleine largeur), puis
/// lance un rafraichissement partiel.
fn send_rows(epd: &mut esp32_quick::Epd, buf: &[u8], y: u32, height: u32) {
    let rows = &buf[y as usize * ROW_BYTES..(y + height) as usize * ROW_BYTES];
    epd.dis_part_ram(0, y, rows, PX_WIDTH, height);
    epd.part_update();
}

/// Dessine une bande noire (avec un texte blanc) sur les lignes `y..y+height`
/// du tampon, puis n'envoie QUE ces lignes au controleur (fenetre pleine
/// largeur) et lance un rafraichissement partiel.
fn send_band(epd: &mut esp32_quick::Epd, buf: &mut [u8], y: u32, height: u32, label: &str) {
    {
        let mut canvas = FrameBuffer { buf: &mut *buf, width: PX_WIDTH, height: PX_HEIGHT };
        let _ = Rectangle::new(Point::new(120, y as i32), Size::new(560, height))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(&mut canvas);
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::Off);
        let _ = Text::new(label, Point::new(140, y as i32 + 26), style).draw(&mut canvas);
    }
    send_rows(epd, buf, y, height);
}
