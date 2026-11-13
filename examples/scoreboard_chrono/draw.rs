//! Dessin d'un tableau de score a l'ancienne : chaque chiffre est une carte
//! entiere (noire, chiffre blanc) accrochee EN HAUT a une barre par deux
//! anneaux. Pas de fente au milieu.
//!
//! Bascule : pour passer a la carte suivante, on souleve la carte de devant
//! par-dessus la barre. Image intermediaire : la nouvelle carte est visible en
//! dessous, l'ancienne est en train de se soulever ; vue de face elle parait
//! ecrasee en hauteur (chiffre compresse) et ne couvre que le haut.
//!
//! Ne depend que d'embedded-graphics (et du FrameBuffer importe a la racine
//! du programme), pour pouvoir aussi etre dessine sur PC en apercu.

use embedded_graphics::draw_target::DrawTargetExt;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, RoundedRectangle};
use embedded_graphics::text::{Baseline, Text};

use crate::FrameBuffer;

/// Taille d'une carte.
const CARD_WIDTH: u32 = 84;
const CARD_HEIGHT: u32 = 140;
const GAP: i32 = 12;
const COLON_WIDTH: u32 = 28;
/// Agrandissement du chiffre : x7 en largeur, x7 en hauteur sur une carte
/// pendante, x3 en hauteur sur une carte en train de se soulever.
const FACTOR_X: u32 = 7;
const FACTOR_Y: u32 = 7;
const FACTOR_Y_LIFTED: u32 = 3;
/// Hauteur visible d'une carte en train de se soulever (vue de face).
const LIFTED_HEIGHT: u32 = CARD_HEIGHT * FACTOR_Y_LIFTED / FACTOR_Y;

/// Barre et anneaux.
const RAIL_THICKNESS: u32 = 6;
const RAIL_GAP: i32 = 22; // espace entre la barre et le haut des cartes
const HOOK_INSET: i32 = 18; // position des anneaux depuis les bords de la carte

fn is_colon(c: char) -> bool {
    c == ':'
}

fn char_width(c: char) -> u32 {
    if is_colon(c) { COLON_WIDTH } else { CARD_WIDTH }
}

fn row_width(text: &str) -> i32 {
    let count = text.chars().count() as i32;
    text.chars().map(|c| char_width(c) as i32).sum::<i32>() + GAP * (count - 1).max(0)
}

/// Dessine le tableau, centre horizontalement, cartes centrees sur `center_y`.
/// `lifting` : si fourni (meme longueur), les cartes qui different sont
/// dessinees en cours de bascule (ancienne carte `lifting` qui se souleve
/// au-dessus de la nouvelle carte `text`).
pub fn draw_scoreboard(canvas: &mut FrameBuffer, text: &str, lifting: Option<&str>, center_y: i32) {
    let lifting = lifting.filter(|old| old.chars().count() == text.chars().count());
    let top = center_y - CARD_HEIGHT as i32 / 2;
    let width = row_width(text);
    let left = (canvas.width as i32 - width) / 2;
    let black = PrimitiveStyle::with_fill(BinaryColor::On);

    // Barre horizontale a laquelle les cartes sont accrochees.
    let rail_y = top - RAIL_GAP;
    let _ = Rectangle::new(Point::new(left - 16, rail_y), Size::new(width as u32 + 32, RAIL_THICKNESS))
        .into_styled(black)
        .draw(canvas);

    let mut x = left;
    for (index, new) in text.chars().enumerate() {
        if is_colon(new) {
            draw_colon(canvas, x, top);
        } else {
            let old = lifting.and_then(|old| old.chars().nth(index)).unwrap_or(new);
            draw_card(canvas, x, top, rail_y, new, old);
        }
        x += char_width(new) as i32 + GAP;
    }
}

/// Une carte pendante (`new`), et par-dessus l'ancienne carte (`old`) en train
/// de se soulever si elle est differente.
fn draw_card(canvas: &mut FrameBuffer, x: i32, top: i32, rail_y: i32, new: char, old: char) {
    let black = PrimitiveStyle::with_fill(BinaryColor::On);
    let white = PrimitiveStyle::with_fill(BinaryColor::Off);
    let card = Rectangle::new(Point::new(x, top), Size::new(CARD_WIDTH, CARD_HEIGHT));

    // Anneaux : de la barre jusqu'au haut de la carte.
    for hook_x in [x + HOOK_INSET, x + CARD_WIDTH as i32 - HOOK_INSET - 4] {
        let _ = Rectangle::new(Point::new(hook_x, rail_y), Size::new(4, (top - rail_y + 10) as u32))
            .into_styled(black)
            .draw(canvas);
    }

    // Nouvelle carte, pendante.
    let _ = RoundedRectangle::with_equal_corners(card, Size::new(6, 6))
        .into_styled(black)
        .draw(canvas);
    draw_glyph(canvas, new, card.center(), FACTOR_Y, card);
    draw_holes(canvas, x, top);

    if old == new {
        return;
    }

    // Ancienne carte en train de se soulever : ecrasee en hauteur, en haut.
    let lifted = Rectangle::new(Point::new(x, top), Size::new(CARD_WIDTH, LIFTED_HEIGHT));
    let _ = lifted.into_styled(black).draw(canvas);
    draw_glyph(canvas, old, lifted.center(), FACTOR_Y_LIFTED, lifted);
    draw_holes(canvas, x, top);
    // Bord inferieur de la carte soulevee : fin liseré blanc, comme une ombre.
    let _ = Rectangle::new(Point::new(x, top + LIFTED_HEIGHT as i32), Size::new(CARD_WIDTH, 3))
        .into_styled(white)
        .draw(canvas);
}

/// Trous des anneaux, en haut de la carte.
fn draw_holes(canvas: &mut FrameBuffer, x: i32, top: i32) {
    let white = PrimitiveStyle::with_fill(BinaryColor::Off);
    for hole_x in [x + HOOK_INSET - 2, x + CARD_WIDTH as i32 - HOOK_INSET - 6] {
        let _ = Rectangle::new(Point::new(hole_x, top + 4), Size::new(8, 6))
            .into_styled(white)
            .draw(canvas);
    }
}

/// Deux points noirs sur le fond blanc.
fn draw_colon(canvas: &mut FrameBuffer, x: i32, top: i32) {
    let dot = 14;
    let dot_x = x + (COLON_WIDTH as i32 - dot) / 2;
    for row in [CARD_HEIGHT as i32 / 3, CARD_HEIGHT as i32 * 2 / 3] {
        let _ = Rectangle::new(Point::new(dot_x, top + row - dot / 2), Size::new(dot as u32, dot as u32))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(canvas);
    }
}

/// Dessine `c` en blanc, centre sur `center`, agrandi FACTOR_X en largeur et
/// `factor_y` en hauteur, limite a `clip`. (Dans sa case de 10x20, un chiffre
/// de FONT_10X20 occupe les colonnes 1 a 8 et les lignes 3 a 15.)
fn draw_glyph(canvas: &mut FrameBuffer, c: char, center: Point, factor_y: u32, clip: Rectangle) {
    let mut text = [0u8; 4];
    let text = c.encode_utf8(&mut text);
    let glyph_center = Point::new(5 * FACTOR_X as i32, 19 * factor_y as i32 / 2);

    let mut clipped = canvas.clipped(&clip);
    let mut translated = clipped.translated(center - glyph_center);
    let mut scaled = ScaledXY { target: &mut translated, factor_x: FACTOR_X, factor_y };
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::Off);
    let _ = Text::with_baseline(text, Point::zero(), style, Baseline::Top).draw(&mut scaled);
}

/// Comme `Scaled` de la bibliotheque, mais avec des facteurs differents en
/// largeur et en hauteur (pour ecraser le chiffre d'une carte soulevee).
struct ScaledXY<'a, T> {
    target: &'a mut T,
    factor_x: u32,
    factor_y: u32,
}

impl<T: Dimensions> Dimensions for ScaledXY<'_, T> {
    fn bounding_box(&self) -> Rectangle {
        let inner = self.target.bounding_box();
        Rectangle::new(
            Point::new(inner.top_left.x / self.factor_x as i32, inner.top_left.y / self.factor_y as i32),
            Size::new(inner.size.width / self.factor_x, inner.size.height / self.factor_y),
        )
    }
}

impl<T: DrawTarget<Color = BinaryColor>> DrawTarget for ScaledXY<'_, T> {
    type Color = BinaryColor;
    type Error = T::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let (fx, fy) = (self.factor_x as i32, self.factor_y as i32);
        let expanded = pixels.into_iter().flat_map(move |Pixel(coord, color)| {
            let (x0, y0) = (coord.x * fx, coord.y * fy);
            (0..fy).flat_map(move |dy| (0..fx).map(move |dx| Pixel(Point::new(x0 + dx, y0 + dy), color)))
        });
        self.target.draw_iter(expanded)
    }
}
