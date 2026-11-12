#![no_std]

//! Bibliotheque partagee entre l'application (`docphone`) et les programmes de
//! test du materiel (`examples/`).
//!
//! - `board`   : cablage et initialisation du materiel
//! - `display` : ecran e-paper, zone de dessin, `Display`
//! - `clock`   : heure et date

pub mod board;
pub mod clock;
pub mod display;

// Raccourcis : `esp32_quick::Epd` plutot que `esp32_quick::display::Epd`.
pub use display::{
    BUF_SIZE, Epd, FrameBuffer, PARTIAL_REFRESH_LIMIT, PX_HEIGHT, PX_WIDTH, Scaled, clear_full_buffer,
};
