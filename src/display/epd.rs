//! Pilote de l'ecran e-paper GDEQ0426T82 (controleur SSD1677).
//!
//! Port direct du driver C fonctionnel (Display_EPD_W21.cpp / Display_EPD_W21_spi.cpp,
//! projet PlatformIO "indust"/"gooddisplay") vers Rust. On reste volontairement au plus
//! pres de la sequence de registres eprouvee plutot que de passer par un crate tiers.

use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::master::Spi;

/// Resolution physique du panneau GDEQ0426T82 (800 horizontal x 480 vertical).
pub const PX_WIDTH: u32 = 800;
pub const PX_HEIGHT: u32 = 480;
pub const BUF_SIZE: usize = (PX_WIDTH as usize * PX_HEIGHT as usize) / 8;

/// Durees constructeur (GDEQ0426T82) : complet 3,5 s, rapide 1,5 s, partiel 0,42 s.
/// S'y ajoute l'envoi de l'image en SPI (48 000 octets, ~0,1 s a 4 MHz).

/// Nombre de rafraichissements partiels avant un rafraichissement complet force.
/// Le partiel laisse des traces (ghosting) qui s'accumulent ; un complet les efface.
pub const PARTIAL_REFRESH_LIMIT: u8 = 10;

/// Limite utilisee par `show_live()` (affichages qui changent chaque seconde :
/// chrono, minuteur). Plus haute pour eviter un clignotement toutes les 10 s ;
/// a 1 Hz, cela fait un rafraichissement complet toutes les 2 minutes environ.
pub const LIVE_PARTIAL_REFRESH_LIMIT: u8 = 120;

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
        self.show_with_limit(buf, PARTIAL_REFRESH_LIMIT);
    }

    /// Comme `show()`, mais pour un affichage qui change en continu (chaque
    /// seconde) : tolere LIVE_PARTIAL_REFRESH_LIMIT partiels avant un complet.
    /// Les traces s'accumulent davantage ; le prochain `show()` normal fera un
    /// rafraichissement complet si la limite normale est depassee.
    pub fn show_live(&mut self, buf: &[u8]) {
        self.show_with_limit(buf, LIVE_PARTIAL_REFRESH_LIMIT);
    }

    fn show_with_limit(&mut self, buf: &[u8], limit: u8) {
        if self.full_pending || self.partial_count >= limit {
            let mut delay = Delay::new();
            self.hw_init(&mut delay);
            self.set_ram_value_basemap(buf);
            self.full_pending = false;
            self.partial_count = 0;
        } else {
            self.dis_part_ram(0, 0, buf, PX_WIDTH, PX_HEIGHT);
            self.part_update();
            self.partial_count = self.partial_count.saturating_add(1);
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

    /// Envoie un bloc de donnees en une seule transaction SPI (CS bas une fois).
    /// Le driver C de reference basculait CS a chaque octet, ce qui est tres lent
    /// pour les 48 000 octets d'une image ; le SSD1677 accepte un flux continu.
    fn write_data_bulk(&mut self, data: &[u8]) {
        self.cs.set_low();
        self.dc.set_high(); // D/C# 1:data
        let _ = self.spi.write(data);
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
        self.write_data_bulk(data);
        self.update();
    }

    /// Port de EPD_WhiteScreen_ALL_Fast().
    pub fn whitescreen_all_fast(&mut self, data: &[u8]) {
        self.write_cmd(0x24);
        self.write_data_bulk(data);
        self.update_fast();
    }

    /// Port de EPD_SetRAMValue_BaseMap() : pose le fond de reference pour le
    /// rafraichissement partiel (a appeler apres hw_init(), avant les dis_part_ram()).
    pub fn set_ram_value_basemap(&mut self, data: &[u8]) {
        self.write_cmd(0x24);
        self.write_data_bulk(data);
        self.write_cmd(0x26);
        self.write_data_bulk(data);
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
        self.write_data_bulk(data);
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
