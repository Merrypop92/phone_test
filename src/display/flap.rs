//! Afficheur a palettes facon panneaux de gare (Solari) : chaque caractere est
//! une plaquette noire aux coins arrondis, caractere en blanc, coupee en deux
//! par la fente horizontale, avec les encoches des charnieres sur les cotes.
//!
//! Animation de bascule : `draw_row(.., Some(ancien_texte), ..)` dessine l'etat
//! intermediaire des palettes qui changent, comme la derniere palette qui tombe
//! sur un vrai panneau : demi-palette du haut = nouveau caractere, demi-palette
//! du bas = caractere qui le precede dans l'ordre des palettes (le 4 pour un 5,
//! le 9 pour un 0), et non l'ancien caractere affiche : si l'affichage a saute
//! une valeur (3 -> 5), l'image reste celle d'un vrai panneau (haut 5 / bas 4).
//! On affiche cette image, puis l'etat final.
//!
//! Police : FONT_10X20 agrandie x6 (dans sa case de 10x20, un chiffre occupe
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

/// Dessine `text` en palettes, centre horizontalement, les plaquettes centrees
/// verticalement sur `center_y`. Si `previous` est fourni (et de meme longueur),
/// les caracteres qui changent sont dessines a mi-bascule.
pub fn draw_row(canvas: &mut FrameBuffer, text: &str, previous: Option<&str>, center_y: i32) {
    let previous = previous.filter(|old| old.chars().count() == text.chars().count());
    let top = center_y - TILE_HEIGHT as i32 / 2;
    let mut x = (canvas.width as i32 - row_width(text)) / 2;

    for (index, new) in text.chars().enumerate() {
        let old = previous.and_then(|old| old.chars().nth(index)).unwrap_or(new);
        let bottom = if old == new { new } else { flap_predecessor(new, old) };
        if is_separator(new) {
            draw_separator(canvas, x, top, new);
        } else {
            draw_tile(canvas, x, top, new, bottom);
        }
        x += char_width(new) as i32 + GAP;
    }
}

/// Palette qui precede `new` sur son tambour (`old` = caractere affiche avant).
/// - 1 a 9 : le chiffre juste avant (meme si l'affichage a saute une valeur) ;
/// - 0 : le tambour vient de faire un tour, et sa derniere valeur depend de la
///   position (9 pour des unites, 5 pour des dizaines de secondes, 2 pour des
///   dizaines d'heures) : c'est l'ancien chiffre affiche ;
/// - autres caracteres : l'ancien caractere.
fn flap_predecessor(new: char, old: char) -> char {
    match new {
        '1'..='9' => char::from_u32(new as u32 - 1).unwrap_or(old),
        _ => old,
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
