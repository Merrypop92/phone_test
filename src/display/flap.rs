//! Afficheur a palettes facon panneaux de gare (Solari) : chaque caractere est
//! une plaquette noire aux coins arrondis, caractere en blanc, coupee en deux
//! par la fente horizontale, avec les encoches des charnieres sur les cotes.
//!
//! Les moitiés haute et basse sont deux choses distinctes : `draw_row` recoit
//! le texte affiche par les demi-palettes du haut et celui des demi-palettes du
//! bas. Pour animer une bascule, l'appelant change d'abord le texte du haut
//! (image 1 : seuls les hauts bougent), puis aligne le bas sur le haut (image 2 :
//! seuls les bas bougent).
//!
//! Police : FONT_10X20 agrandie x7 (dans sa case de 10x20, un chiffre occupe
//! les colonnes 1 a 8 et les lignes 3 a 15 : centre en (4.5, 9)).

use embedded_graphics::draw_target::DrawTargetExt;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, RoundedRectangle};
use embedded_graphics::text::{Baseline, Text};

use super::canvas::{FrameBuffer, Scaled};

/// Agrandissement de la police (caracteres de 70 x 140 px, glyphe visible 56 x 91).
const FACTOR: u32 = 7;
/// Centre du glyphe dans sa case agrandie, en pixels : (4.5 + 0.5, 9 + 0.5) x FACTOR.
const GLYPH_CENTER: Point = Point::new(5 * FACTOR as i32, 19 * FACTOR as i32 / 2);

/// Taille d'une plaquette (glyphe + marges). Le plus long texte prevu,
/// "100:00.0" (chrono arrete apres 100 min), fait 616 px de large (ecran : 800).
pub const TILE_WIDTH: u32 = 8 * FACTOR + 28;
pub const TILE_HEIGHT: u32 = 13 * FACTOR + 49;
const CORNER_RADIUS: u32 = 8;
/// Largeur reservee a un separateur (':' ou '.'), dessine sans plaquette.
const SEPARATOR_WIDTH: u32 = 28;
/// Espace entre deux plaquettes.
const GAP: i32 = 8;

fn is_separator(c: char) -> bool {
    matches!(c, ':' | '.' | ' ')
}

fn char_width(c: char) -> u32 {
    if is_separator(c) { SEPARATOR_WIDTH } else { TILE_WIDTH }
}

/// Largeur totale d'une rangee de palettes, en pixels.
pub fn row_width(text: &str) -> i32 {
    let count = text.chars().count() as i32;
    text.chars().map(|c| char_width(c) as i32).sum::<i32>() + GAP * (count - 1).max(0)
}

/// Dessine une rangee de palettes, centree horizontalement, les plaquettes
/// centrees verticalement sur `center_y` : demi-palettes du haut = `top_text`,
/// demi-palettes du bas = `bottom_text`. Si les deux textes n'ont pas la meme
/// longueur (disposition differente), `top_text` est utilise pour les deux.
pub fn draw_row(canvas: &mut FrameBuffer, top_text: &str, bottom_text: &str, center_y: i32) {
    let same_layout = top_text.chars().count() == bottom_text.chars().count();
    let top = center_y - TILE_HEIGHT as i32 / 2;
    let mut x = (canvas.width as i32 - row_width(top_text)) / 2;

    for (index, top_char) in top_text.chars().enumerate() {
        let bottom_char = if same_layout {
            bottom_text.chars().nth(index).unwrap_or(top_char)
        } else {
            top_char
        };
        if is_separator(top_char) {
            draw_separator(canvas, x, top, top_char);
        } else {
            draw_tile(canvas, x, top, top_char, bottom_char);
        }
        x += char_width(top_char) as i32 + GAP;
    }
}

/// Une plaquette : haut = `top_char`, bas = `bottom_char` (egaux hors animation).
fn draw_tile(canvas: &mut FrameBuffer, x: i32, top: i32, top_char: char, bottom_char: char) {
    let tile = Rectangle::new(Point::new(x, top), Size::new(TILE_WIDTH, TILE_HEIGHT));
    let _ = RoundedRectangle::with_equal_corners(tile, Size::new(CORNER_RADIUS, CORNER_RADIUS))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(canvas);

    let half = TILE_HEIGHT / 2;
    let mid = top + half as i32;
    let upper = Rectangle::new(Point::new(x, top), Size::new(TILE_WIDTH, half));
    let lower = Rectangle::new(Point::new(x, mid), Size::new(TILE_WIDTH, TILE_HEIGHT - half));
    draw_glyph(canvas, tile, upper, top_char);
    draw_glyph(canvas, tile, lower, bottom_char);

    let white = PrimitiveStyle::with_fill(BinaryColor::Off);
    // Fente entre les deux demi-palettes.
    let _ = Rectangle::new(Point::new(x, mid - 1), Size::new(TILE_WIDTH, 3))
        .into_styled(white)
        .draw(canvas);
    // Encoches des charnieres, de chaque cote de la fente.
    for notch_x in [x, x + TILE_WIDTH as i32 - 5] {
        let _ = Rectangle::new(Point::new(notch_x, mid - 7), Size::new(5, 15))
            .into_styled(white)
            .draw(canvas);
    }
}

/// Dessine `c` en blanc, centre sur `tile`, mais seulement dans `clip`.
fn draw_glyph(canvas: &mut FrameBuffer, tile: Rectangle, clip: Rectangle, c: char) {
    let mut text = [0u8; 4];
    let text = c.encode_utf8(&mut text);
    let origin = tile.center() - GLYPH_CENTER;

    let mut clipped = canvas.clipped(&clip);
    let mut translated = clipped.translated(origin);
    let mut scaled = Scaled { target: &mut translated, factor: FACTOR };
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::Off);
    let _ = Text::with_baseline(text, Point::zero(), style, Baseline::Top).draw(&mut scaled);
}

/// ':' ou '.' : points noirs directement sur le fond blanc, sans plaquette.
fn draw_separator(canvas: &mut FrameBuffer, x: i32, top: i32, c: char) {
    let dot = 14;
    let dot_x = x + (SEPARATOR_WIDTH as i32 - dot) / 2;
    let rows: &[i32] = match c {
        ':' => &[TILE_HEIGHT as i32 / 3, TILE_HEIGHT as i32 * 2 / 3],
        '.' => &[TILE_HEIGHT as i32 - 30],
        _ => &[],
    };
    for &row in rows {
        let _ = Rectangle::new(Point::new(dot_x, top + row - dot / 2), Size::new(dot as u32, dot as u32))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(canvas);
    }
}
