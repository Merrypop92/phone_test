#![no_std]
#![no_main]

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp32_quick::{Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, Scaled, clear_full_buffer};
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};
use esp_hal::main;
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    esp_println::println!("PANIC: {}", info);
    loop {}
}

esp_bootloader_esp_idf::esp_app_desc!();

const BUF_SIZE: usize = (PX_WIDTH as usize * PX_HEIGHT as usize) / 8;
static mut FRAME_BUFFER: [u8; BUF_SIZE] = [0xFF; BUF_SIZE];

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let mut delay = Delay::new();

    esp_println::println!("== Init e-paper GDEQ0426T82 (SSD1677, port du driver C) ==");

    // Bus SPI partage avec la carte SD sur cette carte : SCLK=GPIO18, MOSI=GPIO23.
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(4))
            .with_mode(Mode::_0),
    )
    .expect("echec init SPI")
    .with_sck(peripherals.GPIO18)
    .with_mosi(peripherals.GPIO23);

    // Pins confirmees par le driver C de reference : CS=GPIO13, RST=GPIO12.
    let cs = Output::new(peripherals.GPIO13, Level::High, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let rst = Output::new(peripherals.GPIO12, Level::High, OutputConfig::default());
    let busy = Input::new(peripherals.GPIO5, InputConfig::default());

    let mut epd = Epd::new(spi, cs, dc, rst, busy);

    // SAFETY: acces exclusif, une seule fois, pas d'autre reference concurrente ;
    // on redessine/reutilise ce meme buffer entre chaque sequence.
    let buf: &mut [u8] = unsafe { &mut *core::ptr::addr_of_mut!(FRAME_BUFFER) };

    // --- 1. Rafraichissement complet : ecran blanc ---
    esp_println::println!("[1/4] Full refresh : ecran blanc");
    epd.hw_init(&mut delay);
    clear_full_buffer(buf);
    epd.whitescreen_all(buf);
    epd.deep_sleep(&mut delay);
    delay.delay_millis(2000);

    // --- 2. Rafraichissement complet : rectangle + texte ---
    esp_println::println!("[2/4] Full refresh : rectangle + texte");
    epd.hw_init(&mut delay);
    clear_full_buffer(buf);
    {
        let mut target = FrameBuffer { buf, width: PX_WIDTH, height: PX_HEIGHT };
        let _ = Rectangle::new(Point::new(20, 20), Size::new(200, 100))
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 4))
            .draw(&mut target);
        let _ = Text::new(
            "Full refresh",
            Point::new(40, 200),
            MonoTextStyle::new(&FONT_10X20, BinaryColor::On),
        )
        .draw(&mut target);
    }
    epd.whitescreen_all(buf);
    epd.deep_sleep(&mut delay);
    delay.delay_millis(2000);

    // --- 3. Rafraichissement rapide : autre texte ---
    esp_println::println!("[3/4] Fast refresh");
    epd.hw_init_fast(&mut delay);
    clear_full_buffer(buf);
    {
        let mut target = FrameBuffer { buf, width: PX_WIDTH, height: PX_HEIGHT };
        let _ = Text::new(
            "Fast refresh !",
            Point::new(40, 200),
            MonoTextStyle::new(&FONT_10X20, BinaryColor::On),
        )
        .draw(&mut target);
    }
    epd.whitescreen_all_fast(buf);
    epd.deep_sleep(&mut delay);
    delay.delay_millis(2000);

    // --- 4. Rafraichissement partiel : compteur dans une petite zone ---
    esp_println::println!("[4/4] Partial refresh : compteur");
    epd.hw_init(&mut delay);
    clear_full_buffer(buf);
    epd.set_ram_value_basemap(buf); // pose le fond de reference (obligatoire avant du partiel)

    const PART_X: u32 = 280;
    const PART_Y: u32 = 190;
    const PART_W: u32 = 240; // multiple de 8
    const PART_H: u32 = 80;
    const PART_SIZE: usize = (PART_W as usize * PART_H as usize) / 8;

    for i in 0..=5 {
        let mut part_buf = [0xFFu8; PART_SIZE];
        {
            let mut target = FrameBuffer { buf: &mut part_buf, width: PART_W, height: PART_H };
            let mut txt: heapless::String<16> = heapless::String::new();
            let _ = core::fmt::write(&mut txt, format_args!("Compteur: {i}"));
            let _ = Text::new(&txt, Point::new(4, 24), MonoTextStyle::new(&FONT_10X20, BinaryColor::On))
                .draw(&mut target);
        }
        epd.dis_part_ram(PART_X, PART_Y, &part_buf, PART_W, PART_H);
        epd.part_update();
        delay.delay_millis(1000);
    }
    epd.deep_sleep(&mut delay);
    delay.delay_millis(2000);

    // --- Message final (reste affiche, l'e-paper garde l'image sans alimentation) ---
    esp_println::println!("Message final");
    epd.hw_init(&mut delay);
    clear_full_buffer(buf);
    {
        // FONT_10X20 (10x20) agrandie x5 -> caracteres de 50x100px. Sur une largeur
        // de 800px ca ne tient plus sur une seule ligne (25 caracteres), donc deux
        // lignes. Les coordonnees passees a Text::new sont "logiques" (avant mise
        // a l'echelle) : Scaled les multiplie par `factor` a l'ecriture.
        const SCALE: u32 = 5;
        let mut target = FrameBuffer { buf, width: PX_WIDTH, height: PX_HEIGHT };
        let mut scaled = Scaled { target: &mut target, factor: SCALE };
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

        let _ = Text::new("hello", Point::new(25, 44), style).draw(&mut scaled);
        let _ = Text::new("WILLIAM", Point::new(15, 68), style).draw(&mut scaled);
    }
    epd.whitescreen_all(buf);
    epd.deep_sleep(&mut delay);

    esp_println::println!("== Demo terminee ==");
    loop {}
}
