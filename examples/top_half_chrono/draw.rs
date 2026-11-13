//! Chrono "moitie haute" : chaque carte n'affiche que la partie superieure
//! d'un chiffre. Cette partie est un DESSIN (bitmap de 8 x 7 pixels, un bit
//! par pixel), pas un caractere de police : on peut le modifier a la main
//! dans `TOP_HALVES` ci-dessous ('#' = pixel blanc sur la carte noire).
//!
//! Ces dessins ont ete extraits une fois de la police FONT_10X20 (les 7
//! premieres lignes de la zone visible d'un chiffre, jusqu'au milieu), pour
//! garder le meme style que les chiffres de l'application.
//!
//! Ne depend que d'embedded-graphics (et du FrameBuffer importe a la racine
//! du programme), pour pouvoir aussi etre dessine sur PC en apercu.

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, RoundedRectangle};

use crate::FrameBuffer;

/// Dessin de la moitie haute de chaque chiffre, 8 pixels de large x 7 lignes.
/// Bit de gauche = pixel de gauche.
pub const TOP_HALVES: [[u8; 7]; 10] = [
    // '0'
    [
        0b00011000, // ...##...
        0b00111100, // ..####..
        0b01100110, // .##..##.
        0b01100110, // .##..##.
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b11000011, // ##....##
    ],
    // '1'
    [
        0b00011000, // ...##...
        0b00111000, // ..###...
        0b01111000, // .####...
        0b11011000, // ##.##...
        0b00011000, // ...##...
        0b00011000, // ...##...
        0b00011000, // ...##...
    ],
    // '2'
    [
        0b00111100, // ..####..
        0b01100110, // .##..##.
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b00000011, // ......##
        0b00000011, // ......##
        0b00000110, // .....##.
    ],
    // '3'
    [
        0b00111100, // ..####..
        0b01100110, // .##..##.
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b00000011, // ......##
        0b00000110, // .....##.
        0b00011100, // ...###..
    ],
    // '4'
    [
        0b00000010, // ......#.
        0b00000110, // .....##.
        0b00001110, // ....###.
        0b00011110, // ...####.
        0b00110110, // ..##.##.
        0b01100110, // .##..##.
        0b11000110, // ##...##.
    ],
    // '5'
    [
        0b11111111, // ########
        0b11000000, // ##......
        0b11000000, // ##......
        0b11000000, // ##......
        0b11000000, // ##......
        0b11011100, // ##.###..
        0b11100110, // ###..##.
    ],
    // '6'
    [
        0b00111100, // ..####..
        0b01100110, // .##..##.
        0b11000010, // ##....#.
        0b11000000, // ##......
        0b11000000, // ##......
        0b11011100, // ##.###..
        0b11100110, // ###..##.
    ],
    // '7'
    [
        0b11111111, // ########
        0b00000011, // ......##
        0b00000011, // ......##
        0b00000110, // .....##.
        0b00000110, // .....##.
        0b00001100, // ....##..
        0b00001100, // ....##..
    ],
    // '8'
    [
        0b00111100, // ..####..
        0b01100110, // .##..##.
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b01100110, // .##..##.
        0b00111100, // ..####..
    ],
    // '9'
    [
        0b00111100, // ..####..
        0b01100110, // .##..##.
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b01100111, // .##..###
    ],
];

/// Taille d'un pixel du dessin sur l'ecran (dessin de 8 x 7 -> 56 x 49 px).
const PIXEL: u32 = 7;
const CARD_WIDTH: u32 = 84;
const CARD_HEIGHT: u32 = 140;
const GAP: i32 = 12;
const COLON_WIDTH: u32 = 28;

fn char_width(c: char) -> u32 {
    if c == ':' { COLON_WIDTH } else { CARD_WIDTH }
}

/// Dessine `text` ("MM:SS") centre horizontalement, cartes centrees sur
/// `center_y` : une carte noire par chiffre, avec seulement le dessin de la
/// moitie haute du chiffre.
pub fn draw_row(canvas: &mut FrameBuffer, text: &str, center_y: i32) {
    let count = text.chars().count() as i32;
    let width = text.chars().map(|c| char_width(c) as i32).sum::<i32>() + GAP * (count - 1).max(0);
    let top = center_y - CARD_HEIGHT as i32 / 2;
    let mut x = (canvas.width as i32 - width) / 2;

    for c in text.chars() {
        match c.to_digit(10) {
            Some(digit) => draw_card(canvas, x, top, &TOP_HALVES[digit as usize]),
            None if c == ':' => draw_colon(canvas, x, top),
            None => {}
        }
        x += char_width(c) as i32 + GAP;
    }
}

/// Carte noire + dessin de la moitie haute, dont le bas arrive au milieu de
/// la carte (la ou le chiffre complet aurait son milieu).
fn draw_card(canvas: &mut FrameBuffer, x: i32, top: i32, drawing: &[u8; 7]) {
    let card = Rectangle::new(Point::new(x, top), Size::new(CARD_WIDTH, CARD_HEIGHT));
    let _ = RoundedRectangle::with_equal_corners(card, Size::new(6, 6))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(canvas);

    let drawing_width = 8 * PIXEL as i32;
    let drawing_height = drawing.len() as i32 * PIXEL as i32;
    let origin = Point::new(
        x + (CARD_WIDTH as i32 - drawing_width) / 2,
        top + CARD_HEIGHT as i32 / 2 - drawing_height + PIXEL as i32 / 2,
    );
    draw_bitmap(canvas, drawing, origin);
}

/// Dessine un bitmap de 8 pixels de large : chaque bit a 1 devient un carre
/// blanc de PIXEL x PIXEL ; les bits a 0 ne sont pas dessines (la carte noire
/// reste visible).
fn draw_bitmap(canvas: &mut FrameBuffer, rows: &[u8], origin: Point) {
    let white = PrimitiveStyle::with_fill(BinaryColor::Off);
    for (row_index, &row) in rows.iter().enumerate() {
        for column in 0..8 {
            if row & (0x80 >> column) != 0 {
                let position = origin + Point::new(column * PIXEL as i32, row_index as i32 * PIXEL as i32);
                let _ = Rectangle::new(position, Size::new(PIXEL, PIXEL)).into_styled(white).draw(canvas);
            }
        }
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
