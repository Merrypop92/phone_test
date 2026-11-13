//! Dessins des chiffres en deux moities independantes : `TOP_HALVES` (7
//! lignes du haut) et `BOTTOM_HALVES` (6 lignes du bas). Ce sont des bitmaps
//! de 8 pixels de large ('#' = pixel blanc sur la carte noire), modifiables a
//! la main. Haut + bas = chiffre complet (extrait une fois de FONT_10X20).
//!
//! Ne depend que d'embedded-graphics (et du FrameBuffer importe a la racine
//! du programme), pour pouvoir aussi etre dessine sur PC en apercu.

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, RoundedRectangle};

use crate::FrameBuffer;

/// Moitie haute de chaque chiffre : 8 pixels x 7 lignes.
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

/// Moitie basse de chaque chiffre : 8 pixels x 6 lignes.
pub const BOTTOM_HALVES: [[u8; 6]; 10] = [
    // '0'
    [
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b01100110, // .##..##.
        0b01100110, // .##..##.
        0b00111100, // ..####..
        0b00011000, // ...##...
    ],
    // '1'
    [
        0b00011000, // ...##...
        0b00011000, // ...##...
        0b00011000, // ...##...
        0b00011000, // ...##...
        0b00011000, // ...##...
        0b11111111, // ########
    ],
    // '2'
    [
        0b00011100, // ...###..
        0b00110000, // ..##....
        0b01100000, // .##.....
        0b11000000, // ##......
        0b11000000, // ##......
        0b11111111, // ########
    ],
    // '3'
    [
        0b00000110, // .....##.
        0b00000011, // ......##
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b01100110, // .##..##.
        0b00111100, // ..####..
    ],
    // '4'
    [
        0b11000110, // ##...##.
        0b11111111, // ########
        0b00000110, // .....##.
        0b00000110, // .....##.
        0b00000110, // .....##.
        0b00000110, // .....##.
    ],
    // '5'
    [
        0b00000011, // ......##
        0b00000011, // ......##
        0b00000011, // ......##
        0b11000011, // ##....##
        0b01100110, // .##..##.
        0b00111100, // ..####..
    ],
    // '6'
    [
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b01100110, // .##..##.
        0b00111100, // ..####..
    ],
    // '7'
    [
        0b00011000, // ...##...
        0b00011000, // ...##...
        0b00110000, // ..##....
        0b00110000, // ..##....
        0b01100000, // .##.....
        0b01100000, // .##.....
    ],
    // '8'
    [
        0b01100110, // .##..##.
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b11000011, // ##....##
        0b01100110, // .##..##.
        0b00111100, // ..####..
    ],
    // '9'
    [
        0b00111011, // ..###.##
        0b00000011, // ......##
        0b00000011, // ......##
        0b01000011, // .#....##
        0b01100110, // .##..##.
        0b00111100, // ..####..
    ],
];

/// Taille d'un pixel du dessin sur l'ecran (chiffre de 8 x 13 -> 56 x 91 px).
const PIXEL: u32 = 7;
const CARD_WIDTH: u32 = 84;
const CARD_HEIGHT: u32 = 140;
const GAP: i32 = 12;
const COLON_WIDTH: u32 = 28;

/// Ce qu'affiche une carte.
#[derive(Clone, Copy)]
pub enum Face {
    /// Chiffre complet (haut + bas).
    Full(u8),
    /// Haut d'un chiffre et bas d'un autre (pendant un changement : haut du
    /// nouveau chiffre, bas de l'ancien).
    Mixed { top: u8, bottom: u8 },
}

fn char_width(c: char) -> u32 {
    if c == ':' { COLON_WIDTH } else { CARD_WIDTH }
}

/// Dessine `text` ("MM:SS") centre horizontalement, cartes centrees sur
/// `center_y`. `face(index, chiffre)` choisit ce qu'affiche chaque carte.
pub fn draw_row(canvas: &mut FrameBuffer, text: &str, center_y: i32, face: impl Fn(usize, u8) -> Face) {
    let count = text.chars().count() as i32;
    let width = text.chars().map(|c| char_width(c) as i32).sum::<i32>() + GAP * (count - 1).max(0);
    let top = center_y - CARD_HEIGHT as i32 / 2;
    let mut x = (canvas.width as i32 - width) / 2;

    for (index, c) in text.chars().enumerate() {
        match c.to_digit(10) {
            Some(digit) => draw_card(canvas, x, top, face(index, digit as u8)),
            None if c == ':' => draw_colon(canvas, x, top),
            None => {}
        }
        x += char_width(c) as i32 + GAP;
    }
}

/// Carte noire + moitie(s) du chiffre, le chiffre complet etant centre sur la carte.
fn draw_card(canvas: &mut FrameBuffer, x: i32, top: i32, face: Face) {
    let card = Rectangle::new(Point::new(x, top), Size::new(CARD_WIDTH, CARD_HEIGHT));
    let _ = RoundedRectangle::with_equal_corners(card, Size::new(6, 6))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(canvas);

    let px = PIXEL as i32;
    let digit_height = 13 * px;
    let origin = Point::new(
        x + (CARD_WIDTH as i32 - 8 * px) / 2,
        top + (CARD_HEIGHT as i32 - digit_height) / 2,
    );
    let (top_digit, bottom_digit) = match face {
        Face::Full(d) => (d, d),
        Face::Mixed { top, bottom } => (top, bottom),
    };
    draw_bitmap(canvas, &TOP_HALVES[top_digit as usize], origin);
    draw_bitmap(canvas, &BOTTOM_HALVES[bottom_digit as usize], origin + Point::new(0, 7 * px));
}

/// Dessine un bitmap de 8 pixels de large : chaque bit a 1 devient un carre
/// blanc de PIXEL x PIXEL ; les bits a 0 ne sont pas dessines.
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
