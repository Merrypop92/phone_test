//! Zone de dessin : tampon 1 bit/pixel au format du controleur e-paper, utilisable
//! avec embedded-graphics, plus quelques aides pour le texte.

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::Text;

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

/// Police par defaut de l'interface (10x20 px, ASCII uniquement).
pub const CHAR_WIDTH: i32 = 10;

impl FrameBuffer<'_> {
    /// Ecrit `text` en police par defaut ; (x, y) = debut de la ligne de base.
    pub fn text(&mut self, text: &str, x: i32, y: i32) {
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let _ = Text::new(text, Point::new(x, y), style).draw(self);
    }

    /// Ecrit `text` aligne a droite, a `margin` pixels du bord droit.
    pub fn text_right(&mut self, text: &str, margin: i32, y: i32) {
        let x = self.width as i32 - margin - text.len() as i32 * CHAR_WIDTH;
        self.text(text, x, y);
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
