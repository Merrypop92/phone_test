//! Afficheur a palettes facon panneaux de gare (Solari) : chaque caractere est
//! une plaquette noire aux coins arrondis, caractere en blanc, coupee en deux
//! par la fente horizontale, avec les encoches des charnieres sur les cotes.
//!
//! Deux tailles (`FlapStyle`) :
//! - `LARGE` : gros chiffres (heure, chrono...), police agrandie x7 ;
//!   `draw_row` centre une rangee sur l'ecran ; ':' et '.' sont dessines sans
//!   plaquette, en points noirs ;
//! - `SMALL` : texte (menu, titres, listes), police agrandie x2, environ 27
//!   caracteres sur la largeur de l'ecran ; `draw_text` ecrit une ligne a une
//!   position donnee. Tous les caracteres ont une plaquette (espace = palette
//!   vide), comme sur un tableau des departs.
//!
//! Le texte est affiche en MAJUSCULES, lettres accentuees ramenees a leur
//! lettre de base (la police est ASCII) : voir `board_char`.
//!
//! `draw_row_halves` dessine un texte different sur les demi-palettes du haut
//! et du bas (essais de transitions, archives dans examples/ : l'e-paper est
//! trop lent pour animer, l'application n'affiche que des palettes statiques).
//!
//! Police : FONT_10X20 (dans sa case de 10x20, un chiffre ou une majuscule
//! occupe les colonnes 1 a 8 et les lignes 3 a 15 : centre en (4.5, 9)).

use embedded_graphics::draw_target::DrawTargetExt;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, RoundedRectangle};
use embedded_graphics::text::{Baseline, Text};

use super::canvas::{FrameBuffer, Scaled};

/// Dimensions d'une famille de palettes.
pub struct FlapStyle {
    /// Agrandissement de la police (glyphe visible : 8 x 13 pixels de police).
    pub factor: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    corner_radius: u32,
    /// Espace entre deux plaquettes.
    pub gap: i32,
    /// Epaisseur de la fente horizontale.
    slit: u32,
    /// Fente blanche (grand format, effet graphique) ou noire (petit format :
    /// un interstice sombre qui coupe les lettres, comme sur un vrai panneau,
    /// sans tracer de ligne blanche qui ferait paraitre le texte barre).
    white_slit: bool,
    /// Taille des encoches des charnieres (largeur, hauteur) ; (0, 0) = aucune.
    notch: (u32, u32),
    /// ':' et '.' dessines en points sans plaquette (grand format) ; sinon
    /// tous les caracteres ont une plaquette.
    bare_separators: bool,
    /// Largeur reservee a un separateur sans plaquette.
    separator_width: u32,
}

/// Grandes palettes : gros chiffres (glyphe 56 x 91 px, plaquette 84 x 140).
/// Le plus long texte prevu, "100:00.0" (chrono arrete apres 100 min), fait
/// 616 px de large (ecran : 800).
pub const LARGE: FlapStyle = FlapStyle {
    factor: 7,
    tile_width: 8 * 7 + 28,
    tile_height: 13 * 7 + 49,
    corner_radius: 8,
    gap: 8,
    slit: 3,
    white_slit: true,
    notch: (5, 15),
    bare_separators: true,
    separator_width: 28,
};

/// Petites palettes : texte (glyphe 16 x 26 px, plaquette 26 x 44, pas de
/// 30 px : 26 caracteres sur 800 px). Pas d'encoches : a cette taille, elles
/// s'alignent d'une palette a l'autre et le texte parait barre.
pub const SMALL: FlapStyle = FlapStyle {
    factor: 2,
    tile_width: 8 * 2 + 10,
    tile_height: 13 * 2 + 18,
    corner_radius: 3,
    gap: 4,
    slit: 1,
    white_slit: false,
    notch: (0, 0),
    bare_separators: false,
    separator_width: 0,
};

/// Caractere tel qu'affiche sur une palette : majuscule, sans accent ; les
/// caracteres absents de la police deviennent '?'.
pub fn board_char(c: char) -> char {
    let c = match c {
        'à' | 'â' | 'ä' | 'À' | 'Â' | 'Ä' => 'A',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'î' | 'ï' | 'Î' | 'Ï' => 'I',
        'ô' | 'ö' | 'Ô' | 'Ö' => 'O',
        'ù' | 'û' | 'ü' | 'Ù' | 'Û' | 'Ü' => 'U',
        'ç' | 'Ç' => 'C',
        'œ' | 'Œ' => 'O',
        other => other,
    };
    match c.to_ascii_uppercase() {
        printable @ ' '..='~' => printable,
        _ => '?',
    }
}

impl FlapStyle {
    fn is_bare(&self, c: char) -> bool {
        self.bare_separators && matches!(c, ':' | '.')
    }

    fn char_width(&self, c: char) -> u32 {
        if self.is_bare(c) { self.separator_width } else { self.tile_width }
    }

    /// Largeur d'une rangee de palettes, en pixels.
    pub fn text_width(&self, text: &str) -> i32 {
        let count = text.chars().count() as i32;
        text.chars().map(|c| self.char_width(c) as i32).sum::<i32>() + self.gap * (count - 1).max(0)
    }

    /// Nombre de caracteres (avec plaquette) qui tiennent dans `width` pixels.
    pub fn max_chars(&self, width: i32) -> usize {
        ((width + self.gap) / (self.tile_width as i32 + self.gap)).max(0) as usize
    }
}

/// Largeur totale d'une rangee de grandes palettes, en pixels.
pub fn row_width(text: &str) -> i32 {
    LARGE.text_width(text)
}

/// Dessine `text` en grandes palettes, centre horizontalement, les plaquettes
/// centrees verticalement sur `center_y`.
pub fn draw_row(canvas: &mut FrameBuffer, text: &str, center_y: i32) {
    draw_row_halves(canvas, text, text, center_y);
}

/// Comme `draw_row`, mais demi-palettes du haut = `top_text`, du bas =
/// `bottom_text`. Si les deux textes n'ont pas la meme longueur (disposition
/// differente), `top_text` est utilise pour les deux.
pub fn draw_row_halves(canvas: &mut FrameBuffer, top_text: &str, bottom_text: &str, center_y: i32) {
    let style = &LARGE;
    let same_layout = top_text.chars().count() == bottom_text.chars().count();
    let top = center_y - style.tile_height as i32 / 2;
    let mut x = (canvas.width as i32 - style.text_width(top_text)) / 2;

    for (index, top_char) in top_text.chars().enumerate() {
        let bottom_char = if same_layout {
            bottom_text.chars().nth(index).unwrap_or(top_char)
        } else {
            top_char
        };
        if style.is_bare(top_char) {
            draw_separator(canvas, style, x, top, top_char);
        } else {
            draw_tile(canvas, style, x, top, board_char(top_char), board_char(bottom_char));
        }
        x += style.char_width(top_char) as i32 + style.gap;
    }
}

/// Ecrit `text` en palettes de style `style`, a partir de (`x`, `top`) (coin
/// haut gauche de la premiere plaquette). Renvoie la largeur dessinee.
pub fn draw_text(canvas: &mut FrameBuffer, style: &FlapStyle, text: &str, x: i32, top: i32) -> i32 {
    let mut cursor = x;
    for c in text.chars() {
        if style.is_bare(c) {
            draw_separator(canvas, style, cursor, top, c);
        } else {
            let c = board_char(c);
            draw_tile(canvas, style, cursor, top, c, c);
        }
        cursor += style.char_width(c) as i32 + style.gap;
    }
    cursor - x - if text.is_empty() { 0 } else { style.gap }
}

/// Ecrit `text` sur plusieurs lignes de `max_chars` caracteres au plus,
/// coupees aux espaces (un mot plus long qu'une ligne est coupe), espacees de
/// `line_pitch` pixels, sans depasser `max_lines` lignes. Renvoie le nombre de
/// lignes dessinees.
pub fn draw_text_wrapped(
    canvas: &mut FrameBuffer,
    style: &FlapStyle,
    text: &str,
    x: i32,
    top: i32,
    max_chars: usize,
    line_pitch: i32,
    max_lines: usize,
) -> usize {
    let mut lines = 0;
    let mut line = heapless::String::<128>::new();
    let mut line_len = 0;

    let mut flush = |line: &mut heapless::String<128>, line_len: &mut usize, lines: &mut usize| {
        if *lines < max_lines && !line.is_empty() {
            draw_text(canvas, style, line.as_str(), x, top + *lines as i32 * line_pitch);
            *lines += 1;
        }
        line.clear();
        *line_len = 0;
    };

    for word in text.split_whitespace() {
        let mut word = word;
        loop {
            let word_len = word.chars().count();
            let needed = if line_len == 0 { word_len } else { line_len + 1 + word_len };
            if needed <= max_chars {
                if line_len > 0 {
                    let _ = line.push(' ');
                }
                let _ = line.push_str(word);
                line_len = needed;
                break;
            }
            if line_len > 0 {
                flush(&mut line, &mut line_len, &mut lines);
                continue;
            }
            // Mot plus long qu'une ligne : on le coupe.
            let split = word.char_indices().nth(max_chars).map_or(word.len(), |(i, _)| i);
            let _ = line.push_str(&word[..split]);
            line_len = max_chars;
            flush(&mut line, &mut line_len, &mut lines);
            word = &word[split..];
            if word.is_empty() {
                break;
            }
        }
    }
    flush(&mut line, &mut line_len, &mut lines);
    lines
}

/// Marge laterale commune des ecrans.
pub const SCREEN_MARGIN: i32 = 20;
/// Haut de l'en-tete des ecrans.
pub const HEADER_TOP: i32 = 16;
/// Premiere ligne disponible sous l'en-tete et son filet.
pub const CONTENT_TOP: i32 = HEADER_TOP + 44 + 8 + 3;

/// En-tete commun des ecrans : `title` en petites palettes a gauche, `right`
/// (optionnel) aligne a droite, puis un filet noir sur toute la largeur.
/// Le contenu de l'ecran commence a `CONTENT_TOP`.
pub fn draw_header(canvas: &mut FrameBuffer, title: &str, right: Option<&str>) {
    draw_text(canvas, &SMALL, title, SCREEN_MARGIN, HEADER_TOP);
    if let Some(right) = right {
        let x = canvas.width as i32 - SCREEN_MARGIN - SMALL.text_width(right);
        draw_text(canvas, &SMALL, right, x, HEADER_TOP);
    }
    draw_header_rule(canvas);
}

/// Filet noir sous l'en-tete (pour un en-tete dessine a la main).
pub fn draw_header_rule(canvas: &mut FrameBuffer) {
    let rule_y = HEADER_TOP + SMALL.tile_height as i32 + 8;
    let _ = Rectangle::new(Point::new(SCREEN_MARGIN, rule_y), Size::new(canvas.width - 2 * SCREEN_MARGIN as u32, 3))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(canvas);
}

/// Comme `draw_text`, centre horizontalement sur l'ecran.
pub fn draw_text_centered(canvas: &mut FrameBuffer, style: &FlapStyle, text: &str, top: i32) {
    let x = (canvas.width as i32 - style.text_width(text)) / 2;
    draw_text(canvas, style, text, x, top);
}

/// Une plaquette : haut = `top_char`, bas = `bottom_char` (egaux hors essais
/// de transition). Un espace donne une palette vide.
fn draw_tile(canvas: &mut FrameBuffer, style: &FlapStyle, x: i32, top: i32, top_char: char, bottom_char: char) {
    let tile = Rectangle::new(Point::new(x, top), Size::new(style.tile_width, style.tile_height));
    let radius = Size::new(style.corner_radius, style.corner_radius);
    let _ = RoundedRectangle::with_equal_corners(tile, radius)
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(canvas);

    let half = style.tile_height / 2;
    let mid = top + half as i32;
    let upper = Rectangle::new(Point::new(x, top), Size::new(style.tile_width, half));
    let lower = Rectangle::new(Point::new(x, mid), Size::new(style.tile_width, style.tile_height - half));
    draw_glyph(canvas, style, tile, upper, top_char);
    draw_glyph(canvas, style, tile, lower, bottom_char);

    let white = PrimitiveStyle::with_fill(BinaryColor::Off);
    let slit_color = if style.white_slit { BinaryColor::Off } else { BinaryColor::On };
    // Fente entre les deux demi-palettes.
    let slit = style.slit as i32;
    let _ = Rectangle::new(Point::new(x, mid - slit / 2), Size::new(style.tile_width, style.slit))
        .into_styled(PrimitiveStyle::with_fill(slit_color))
        .draw(canvas);
    // Encoches des charnieres, de chaque cote de la fente.
    let (notch_w, notch_h) = style.notch;
    if notch_w == 0 {
        return;
    }
    for notch_x in [x, x + (style.tile_width - notch_w) as i32] {
        let _ = Rectangle::new(Point::new(notch_x, mid - notch_h as i32 / 2), Size::new(notch_w, notch_h))
            .into_styled(white)
            .draw(canvas);
    }
}

/// Dessine `c` en blanc, centre sur `tile`, mais seulement dans `clip`.
fn draw_glyph(canvas: &mut FrameBuffer, style: &FlapStyle, tile: Rectangle, clip: Rectangle, c: char) {
    if c == ' ' {
        return;
    }
    let mut text = [0u8; 4];
    let text = c.encode_utf8(&mut text);
    let f = style.factor as i32;
    let glyph_center = Point::new(5 * f, 19 * f / 2);
    let origin = tile.center() - glyph_center;

    let mut clipped = canvas.clipped(&clip);
    let mut translated = clipped.translated(origin);
    let mut scaled = Scaled { target: &mut translated, factor: style.factor };
    let font = MonoTextStyle::new(&FONT_10X20, BinaryColor::Off);
    let _ = Text::with_baseline(text, Point::zero(), font, Baseline::Top).draw(&mut scaled);
}

/// ':' ou '.' (grand format) : points noirs directement sur le fond blanc.
fn draw_separator(canvas: &mut FrameBuffer, style: &FlapStyle, x: i32, top: i32, c: char) {
    let height = style.tile_height as i32;
    let dot = 2 * style.factor as i32;
    let dot_x = x + (style.separator_width as i32 - dot) / 2;
    let rows: &[i32] = match c {
        ':' => &[height / 3, height * 2 / 3],
        '.' => &[height - 30],
        _ => &[],
    };
    for &row in rows {
        let _ = Rectangle::new(Point::new(dot_x, top + row - dot / 2), Size::new(dot as u32, dot as u32))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(canvas);
    }
}
