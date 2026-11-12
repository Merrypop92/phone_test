#![no_std]

//! Driver e-paper partage entre l'application (`docphone`) et les exemples (`epaper_demo`, ...).
//!
//! Port direct du driver C fonctionnel (Display_EPD_W21.cpp / Display_EPD_W21_spi.cpp,
//! projet PlatformIO "indust"/"gooddisplay") vers Rust. On reste volontairement au plus
//! pres de la sequence de registres eprouvee plutot que de passer par un crate tiers.

pub mod clock;

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::master::Spi;

/// Resolution physique du panneau GDEQ0426T82 (800 horizontal x 480 vertical).
pub const PX_WIDTH: u32 = 800;
pub const PX_HEIGHT: u32 = 480;
pub const BUF_SIZE: usize = (PX_WIDTH as usize * PX_HEIGHT as usize) / 8;

/// Nombre de rafraichissements partiels avant un rafraichissement complet force.
/// Le partiel laisse des traces (ghosting) qui s'accumulent ; un complet les efface.
pub const PARTIAL_REFRESH_LIMIT: u8 = 10;

/// Driver SSD1677 minimal, port 1:1 des fonctions EPD_* du driver C de reference.
pub struct Epd<'d> {
    spi: Spi<'d, Blocking>,
    cs: Output<'d>,
    dc: Output<'d>,
    rst: Output<'d>,
    busy: Input<'d>,
    /// Rafraichissements partiels depuis le dernier complet.
    partial_count: u8,
    /// Vrai si le prochain `show()` doit etre un rafraichissement complet.
    full_pending: bool,
}

impl<'d> Epd<'d> {
    pub fn new(spi: Spi<'d, Blocking>, cs: Output<'d>, dc: Output<'d>, rst: Output<'d>, busy: Input<'d>) -> Self {
        Self { spi, cs, dc, rst, busy, partial_count: 0, full_pending: true }
    }

    /// Demande que le prochain `show()` fasse un rafraichissement complet
    /// (changement d'ecran, nettoyage des traces).
    pub fn request_full_refresh(&mut self) {
        self.full_pending = true;
    }

    /// Affiche un buffer plein ecran en choisissant le mode de rafraichissement :
    /// - complet (lent, clignote) si demande, au premier affichage, ou apres
    ///   PARTIAL_REFRESH_LIMIT partiels ; il pose aussi le fond de reference
    ///   (RAM 0x24 + 0x26) indispensable au partiel ;
    /// - partiel (rapide, sans clignotement) sinon.
    pub fn show(&mut self, buf: &[u8]) {
        if self.full_pending || self.partial_count >= PARTIAL_REFRESH_LIMIT {
            let mut delay = Delay::new();
            self.hw_init(&mut delay);
            self.set_ram_value_basemap(buf);
            self.full_pending = false;
            self.partial_count = 0;
        } else {
            self.dis_part_ram(0, 0, buf, PX_WIDTH, PX_HEIGHT);
            self.part_update();
            self.partial_count += 1;
        }
    }

    fn write_cmd(&mut self, cmd: u8) {
        self.cs.set_low();
        self.dc.set_low(); // D/C# 0:command
        let _ = self.spi.write(&[cmd]);
        self.cs.set_high();
    }

    fn write_data(&mut self, data: u8) {
        self.cs.set_low();
        self.dc.set_high(); // D/C# 1:data
        let _ = self.spi.write(&[data]);
        self.cs.set_high();
    }

    fn wait_busy(&mut self) {
        while self.busy.is_high() {}
    }

    fn hw_reset(&mut self, delay: &mut Delay) {
        self.rst.set_low();
        delay.delay_millis(10);
        self.rst.set_high();
        delay.delay_millis(10);
    }

    /// Registres communs a EPD_HW_Init() et EPD_HW_Init_Fast() (0x18 a 0x4F).
    /// Les constantes 480/800 ci-dessous reprennent EPD_WIDTH/EPD_HEIGHT du code C
    /// original (noms trompeurs chez le fabricant : EPD_WIDTH=480 est en fait l'axe
    /// vertical/gate, EPD_HEIGHT=800 l'axe horizontal/pixel). On garde les memes
    /// valeurs brutes que la reference qui fonctionne.
    fn init_common_registers(&mut self) {
        self.write_cmd(0x18); // capteur de temperature interne
        self.write_data(0x80);

        self.write_cmd(0x0C); // booster soft-start
        self.write_data(0xAE);
        self.write_data(0xC7);
        self.write_data(0xC3);
        self.write_data(0xC0);
        self.write_data(0x80);

        self.write_cmd(0x01); // Driver output control
        self.write_data(((480 - 1) % 256) as u8);
        self.write_data(((480 - 1) / 256) as u8);
        self.write_data(0x02);

        self.write_cmd(0x3C); // BorderWaveform
        self.write_data(0x01);

        self.write_cmd(0x11); // data entry mode
        self.write_data(0x03);

        self.write_cmd(0x44); // set RAM-X address start/end
        self.write_data(0x00);
        self.write_data(0x00);
        self.write_data(((800 - 1) % 256) as u8);
        self.write_data(((800 - 1) / 256) as u8);

        self.write_cmd(0x45); // set RAM-Y address start/end
        self.write_data(0x00);
        self.write_data(0x00);
        self.write_data(((480 - 1) % 256) as u8);
        self.write_data(((480 - 1) / 256) as u8);

        self.write_cmd(0x4E); // RAM-X address counter = 0
        self.write_data(0x00);
        self.write_data(0x00);
        self.write_cmd(0x4F); // RAM-Y address counter = 0
        self.write_data(0x00);
        self.write_data(0x00);
    }

    /// Port de EPD_HW_Init() : initialisation pour rafraichissement complet.
    pub fn hw_init(&mut self, delay: &mut Delay) {
        self.hw_reset(delay);

        self.wait_busy();
        self.write_cmd(0x12); // SWRESET
        self.wait_busy();

        self.init_common_registers();
        self.wait_busy();
    }

    /// Port de EPD_HW_Init_Fast() : initialisation + activation du mode rafraichissement rapide.
    pub fn hw_init_fast(&mut self, delay: &mut Delay) {
        self.hw_reset(delay);

        self.wait_busy();
        self.write_cmd(0x12); // SWRESET
        self.wait_busy();

        self.init_common_registers();
        self.wait_busy();

        self.write_cmd(0x1A); // TEMP (1.5s)
        self.write_data(0x5A);

        self.write_cmd(0x22);
        self.write_data(0x91);
        self.write_cmd(0x20);
        self.wait_busy();
    }

    /// Port de EPD_Update() : declenche un rafraichissement complet.
    pub fn update(&mut self) {
        self.write_cmd(0x22); // Display Update Control
        self.write_data(0xF7);
        self.write_cmd(0x20); // Activate Display Update Sequence
        self.wait_busy();
    }

    /// Port de EPD_Update_Fast().
    pub fn update_fast(&mut self) {
        self.write_cmd(0x22);
        self.write_data(0xC7);
        self.write_cmd(0x20);
        self.wait_busy();
    }

    /// Port de EPD_Part_Update().
    pub fn part_update(&mut self) {
        self.write_cmd(0x22);
        self.write_data(0xFF);
        self.write_cmd(0x20);
        self.wait_busy();
    }

    /// Port de EPD_WhiteScreen_ALL() : envoie un buffer complet puis rafraichit.
    pub fn whitescreen_all(&mut self, data: &[u8]) {
        self.write_cmd(0x24); // write RAM, noir(0)/blanc(1)
        for &b in data {
            self.write_data(b);
        }
        self.update();
    }

    /// Port de EPD_WhiteScreen_ALL_Fast().
    pub fn whitescreen_all_fast(&mut self, data: &[u8]) {
        self.write_cmd(0x24);
        for &b in data {
            self.write_data(b);
        }
        self.update_fast();
    }

    /// Port de EPD_SetRAMValue_BaseMap() : pose le fond de reference pour le
    /// rafraichissement partiel (a appeler apres hw_init(), avant les dis_part_ram()).
    pub fn set_ram_value_basemap(&mut self, data: &[u8]) {
        self.write_cmd(0x24);
        for &b in data {
            self.write_data(b);
        }
        self.write_cmd(0x26);
        for &b in data {
            self.write_data(b);
        }
        self.update();
    }

    /// Port de EPD_Dis_Part_RAM() : ecrit une sous-region dans la RAM du controleur,
    /// sans declencher le rafraichissement (appeler part_update() ensuite).
    /// x/width sur l'axe horizontal (800px), y/height sur l'axe vertical (480px).
    pub fn dis_part_ram(&mut self, x: u32, y: u32, data: &[u8], width: u32, height: u32) {
        let x_start = x - (x % 8); // alignement 8px requis par le controleur
        let x_end = x_start + width - 1;
        let y_start = y;
        let y_end = y_start + height - 1;

        self.write_cmd(0x18);
        self.write_data(0x80);

        self.write_cmd(0x3C); // BorderWaveform
        self.write_data(0x80);

        self.write_cmd(0x44); // set RAM-X address start/end
        self.write_data((x_start % 256) as u8);
        self.write_data((x_start / 256) as u8);
        self.write_data((x_end % 256) as u8);
        self.write_data((x_end / 256) as u8);

        self.write_cmd(0x45); // set RAM-Y address start/end
        self.write_data((y_start % 256) as u8);
        self.write_data((y_start / 256) as u8);
        self.write_data((y_end % 256) as u8);
        self.write_data((y_end / 256) as u8);

        self.write_cmd(0x4E);
        self.write_data((x_start % 256) as u8);
        self.write_data((x_start / 256) as u8);
        self.write_cmd(0x4F);
        self.write_data((y_start % 256) as u8);
        self.write_data((y_start / 256) as u8);

        self.write_cmd(0x24);
        for &b in data {
            self.write_data(b);
        }
    }

    /// Port de EPD_Dis_Part() : comme dis_part_ram() mais fait le reset + declenche
    /// elle-meme le rafraichissement partiel (une seule region).
    #[allow(dead_code)]
    pub fn dis_part(&mut self, delay: &mut Delay, x: u32, y: u32, data: &[u8], width: u32, height: u32) {
        self.hw_reset(delay);
        self.dis_part_ram(x, y, data, width, height);
        self.part_update();
    }

    /// Port de EPD_DeepSleep().
    pub fn deep_sleep(&mut self, delay: &mut Delay) {
        self.write_cmd(0x10);
        self.write_data(0x01);
        delay.delay_millis(100);
    }
}

/// Cible de dessin embedded-graphics generique (pleine page ou sous-region), au
/// format attendu par le controleur (1 bit/pixel, 1=blanc, 0=noir, ligne par
/// ligne, MSB = pixel le plus a gauche). `width` doit etre multiple de 8.
pub struct FrameBuffer<'a> {
    pub buf: &'a mut [u8],
    pub width: u32,
    pub height: u32,
}

impl OriginDimensions for FrameBuffer<'_> {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

impl DrawTarget for FrameBuffer<'_> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let bytes_per_row = (self.width / 8) as usize;
        for Pixel(coord, color) in pixels {
            if coord.x < 0 || coord.y < 0 || coord.x as u32 >= self.width || coord.y as u32 >= self.height {
                continue;
            }
            // Le panneau affiche en miroir horizontal par rapport a notre convention
            // de dessin ; on compense ici plutot que de toucher a l'init du controleur.
            let x = (self.width - 1 - coord.x as u32) as usize;
            let y = coord.y as usize;
            let idx = y * bytes_per_row + x / 8;
            let bit = 7 - (x % 8);
            if color == BinaryColor::On {
                self.buf[idx] &= !(1 << bit); // noir
            } else {
                self.buf[idx] |= 1 << bit; // blanc
            }
        }
        Ok(())
    }
}

/// Remplit `buf` (plein ecran) en blanc.
pub fn clear_full_buffer(buf: &mut [u8]) {
    buf.fill(0xFF);
}

/// Cible de dessin qui agrandit chaque pixel en bloc `factor`x`factor` avant de
/// le transmettre a la cible sous-jacente. embedded-graphics n'a pas de mise a
/// l'echelle integree pour le texte, donc on la fait nous-memes.
pub struct Scaled<'a, T> {
    pub target: &'a mut T,
    pub factor: u32,
}

impl<T: Dimensions> Dimensions for Scaled<'_, T> {
    fn bounding_box(&self) -> Rectangle {
        let inner = self.target.bounding_box();
        Rectangle::new(inner.top_left / self.factor as i32, inner.size / self.factor)
    }
}

impl<T: DrawTarget<Color = BinaryColor>> DrawTarget for Scaled<'_, T> {
    type Color = BinaryColor;
    type Error = T::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let factor = self.factor as i32;
        let expanded = pixels.into_iter().flat_map(move |Pixel(coord, color)| {
            let x0 = coord.x * factor;
            let y0 = coord.y * factor;
            (0..factor).flat_map(move |dy| (0..factor).map(move |dx| Pixel(Point::new(x0 + dx, y0 + dy), color)))
        });
        self.target.draw_iter(expanded)
    }
}
