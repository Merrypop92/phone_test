//! Affichage : pilote e-paper (`epd`), zone de dessin (`canvas`) et `Display`,
//! qui regroupe l'ecran et son tampon d'image.

pub mod canvas;
pub mod epd;

pub use canvas::{CHAR_WIDTH, FrameBuffer, Scaled, clear_full_buffer};
pub use epd::{BUF_SIZE, Epd, LIVE_PARTIAL_REFRESH_LIMIT, PARTIAL_REFRESH_LIMIT, PX_HEIGHT, PX_WIDTH};

/// L'ecran et son tampon d'image plein ecran. Les apps ne voient que la zone
/// de dessin (`FrameBuffer`) : vider le tampon et l'envoyer a l'ecran se fait ici.
pub struct Display<'d> {
    epd: Epd<'d>,
    buf: &'d mut [u8],
}

impl<'d> Display<'d> {
    pub fn new(epd: Epd<'d>, buf: &'d mut [u8]) -> Self {
        Self { epd, buf }
    }

    /// Demande que le prochain `draw()` fasse un rafraichissement complet.
    pub fn request_full_refresh(&mut self) {
        self.epd.request_full_refresh();
    }

    /// Vide le tampon, laisse `render` dessiner dedans, puis l'affiche
    /// (partiel ou complet selon `Epd::show`).
    pub fn draw(&mut self, render: impl FnOnce(&mut FrameBuffer)) {
        self.render_into_buffer(render);
        self.epd.show(self.buf);
    }

    /// Comme `draw()`, pour un affichage qui change chaque seconde (voir
    /// `Epd::show_live`) : moins de rafraichissements complets.
    pub fn draw_live(&mut self, render: impl FnOnce(&mut FrameBuffer)) {
        self.render_into_buffer(render);
        self.epd.show_live(self.buf);
    }

    fn render_into_buffer(&mut self, render: impl FnOnce(&mut FrameBuffer)) {
        clear_full_buffer(self.buf);
        render(&mut FrameBuffer {
            buf: self.buf,
            width: PX_WIDTH,
            height: PX_HEIGHT,
        });
    }
}
