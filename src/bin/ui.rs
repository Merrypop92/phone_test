#![no_std]
#![no_main]

// Point de depart tres simple pour l'UI du telephone : ecran d'accueil avec une
// horloge qui se met a jour toute seule (refresh partiel, une fois par minute).
//
// Attention : pas de RTC ni de sync reseau pour l'instant (LTE en pause), donc
// c'est un temps ecoule depuis le boot (part de 00:00), pas l'heure reelle.
//
// Prochaines etapes possibles (volontairement pas faites ici) :
//   - lire la vraie tension batterie (ADC GPIO39, cf. src/bin/main.rs) au lieu
//     du texte "100%" fixe
//   - recaler l'horloge sur une vraie heure (AT+CCLK? une fois le LTE relance,
//     ou une puce RTC dediee)
//   - naviguer dans le menu avec la matrice de boutons (TCA8418)
//   - vrai deep sleep MCU (RTC timer wakeup) entre deux ticks au lieu d'une
//     boucle active : la ou est le vrai gain batterie, l'ecran lui-meme ne
//     consomme presque rien entre deux refresh partiels
//   - decouper cet ecran en un vrai petit framework (Screen trait, etc.)

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp32_quick::{Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, clear_full_buffer};
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};
use esp_hal::main;
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::{Instant, Rate};

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    esp_println::println!("PANIC: {}", info);
    loop {}
}

esp_bootloader_esp_idf::esp_app_desc!();

const BUF_SIZE: usize = (PX_WIDTH as usize * PX_HEIGHT as usize) / 8;
static mut FRAME_BUFFER: [u8; BUF_SIZE] = [0xFF; BUF_SIZE];

const MENU_ITEMS: [&str; 4] = ["Messages", "Contacts", "Reglages", "Eteindre"];

// Zone horloge dans la barre de statut (multiple de 8 pour la largeur, requis
// par le controleur). Position absolue sur l'ecran, utilisee a la fois pour le
// dessin initial (plein ecran) et pour les refresh partiels ulterieurs.
const CLOCK_X: u32 = 380;
const CLOCK_Y: u32 = 2;
const CLOCK_W: u32 = 88;
const CLOCK_H: u32 = 32;
const CLOCK_SIZE: usize = (CLOCK_W as usize * CLOCK_H as usize) / 8;

/// Formate le temps ecoule depuis le boot en "HH:MM" (pas l'heure reelle, cf. commentaire en tete de fichier).
fn format_clock(elapsed_secs: u64) -> heapless::String<8> {
    let total_minutes = elapsed_secs / 60;
    let hh = (total_minutes / 60) % 24;
    let mm = total_minutes % 60;
    let mut s = heapless::String::new();
    let _ = core::fmt::write(&mut s, format_args!("{hh:02}:{mm:02}"));
    s
}

fn draw_home_screen(target: &mut FrameBuffer, elapsed_secs: u64) {
    let big = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let small = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    // --- Barre de statut ---
    let _ = Text::new("DocPhone", Point::new(10, 24), big).draw(target);
    // Meme decalage local (+4,+24 par rapport a CLOCK_X/CLOCK_Y) que dans update_clock(),
    // pour que le refresh partiel s'aligne exactement sur ce que le plein ecran a trace.
    let clock_txt = format_clock(elapsed_secs);
    let _ = Text::new(&clock_txt, Point::new(CLOCK_X as i32 + 4, CLOCK_Y as i32 + 24), big).draw(target);
    let _ = Text::new("100%", Point::new(720, 24), small).draw(target);
    let _ = Rectangle::new(Point::new(0, 36), Size::new(PX_WIDTH, 2))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(target);

    // --- Menu ---
    let item_height = 60;
    let start_y = 70;
    for (i, label) in MENU_ITEMS.iter().enumerate() {
        let y = start_y + i as i32 * item_height;
        if i == 0 {
            // Premier item "selectionne" (prefigure une future navigation au bouton).
            let _ = Rectangle::new(Point::new(10, y - 24), Size::new(300, 40))
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                .draw(target);
        }
        let _ = Text::new(label, Point::new(24, y), big).draw(target);
    }
}

/// Redessine uniquement la zone horloge et declenche un refresh partiel.
/// A appeler seulement apres set_ram_value_basemap() (sinon le reste de l'ecran
/// se dereglerait au premier refresh partiel).
fn update_clock(epd: &mut Epd, elapsed_secs: u64) {
    let mut part_buf = [0xFFu8; CLOCK_SIZE];
    {
        let mut target = FrameBuffer { buf: &mut part_buf, width: CLOCK_W, height: CLOCK_H };
        let big = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let clock_txt = format_clock(elapsed_secs);
        let _ = Text::new(&clock_txt, Point::new(4, 24), big).draw(&mut target);
    }
    epd.dis_part_ram(CLOCK_X, CLOCK_Y, &part_buf, CLOCK_W, CLOCK_H);
    epd.part_update();
}

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let mut delay = Delay::new();

    esp_println::println!("== UI : ecran d'accueil ==");

    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(4))
            .with_mode(Mode::_0),
    )
    .expect("echec init SPI")
    .with_sck(peripherals.GPIO18)
    .with_mosi(peripherals.GPIO23);

    let cs = Output::new(peripherals.GPIO13, Level::High, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let rst = Output::new(peripherals.GPIO12, Level::High, OutputConfig::default());
    let busy = Input::new(peripherals.GPIO5, InputConfig::default());

    let mut epd = Epd::new(spi, cs, dc, rst, busy);

    // SAFETY: acces exclusif, une seule fois.
    let buf: &mut [u8] = unsafe { &mut *core::ptr::addr_of_mut!(FRAME_BUFFER) };

    epd.hw_init(&mut delay);
    clear_full_buffer(buf);
    {
        let mut target = FrameBuffer { buf, width: PX_WIDTH, height: PX_HEIGHT };
        draw_home_screen(&mut target, 0);
    }

    esp_println::println!("Affichage...");
    epd.whitescreen_all(buf);
    // Reference pour les futurs refresh partiels de l'horloge (obligatoire, sinon
    // chaque tick fait deriver visuellement le reste de l'ecran).
    epd.set_ram_value_basemap(buf);
    esp_println::println!("== Ecran affiche, horloge en marche ==");

    // Pas de deep_sleep() ici : le controleur doit rester reveille pour accepter
    // les refresh partiels (un deep sleep exigerait un hw_init() complet a chaque
    // tick, ce qui annulerait l'interet du refresh partiel).
    let start = Instant::now();
    let mut last_minute = 0u64;
    loop {
        let elapsed_secs = start.elapsed().as_secs();
        let minute = elapsed_secs / 60;
        if minute != last_minute {
            last_minute = minute;
            update_clock(&mut epd, elapsed_secs);
        }
        delay.delay_millis(1000);
    }
}
