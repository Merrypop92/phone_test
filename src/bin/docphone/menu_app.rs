use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Triangle};
use esp32_quick::FrameBuffer;
use esp32_quick::clock;
use esp32_quick::system;
use esp32_quick::display::flap::{self, SMALL};
use esp32_quick::input::{Input, Key};
use heapless::String as HeapString;

use super::app::AppCommand;

/// Mise en page du tableau des departs.
const ARROW_LEFT: i32 = 30;
const LIST_TOP: i32 = 120;
const LIST_LEFT: i32 = 80;
/// Ecart vertical entre deux lignes (plaquettes de 44 px) : 5 apps visibles.
const ROW_PITCH: i32 = 64;

/// Fleche noire pointant vers la droite, pointe a (x + 30, center_y).
fn draw_arrow(canvas: &mut FrameBuffer, x: i32, center_y: i32) {
    let _ = Triangle::new(
        Point::new(x, center_y - 16),
        Point::new(x, center_y + 16),
        Point::new(x + 30, center_y),
    )
    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
    .draw(canvas);
}

/// Menu principal. La liste des apps vient du launcher (registre de main.rs) :
/// le menu ne garde que la position de la selection.
pub struct MenuApp {
    selected: usize,
    /// Minute affichee dans l'en-tete (pour redessiner quand elle change).
    shown_minute: Option<u64>,
}

impl MenuApp {
    pub fn new() -> Self {
        Self {
            selected: 0,
            shown_minute: None,
        }
    }

    /// Redessine quand la minute change (ou quand l'heure vient d'etre reglee).
    pub fn tick(&mut self) -> AppCommand {
        let minute = clock::now().map(|now| now.to_secs() / 60);
        if minute == self.shown_minute {
            return AppCommand::Stay;
        }
        self.shown_minute = minute;
        AppCommand::Redraw
    }

    /// Tableau des departs : "DOCPHONE" et la date en petites palettes en
    /// haut, puis un nom d'app par ligne, la ligne selectionnee marquee par une
    /// fleche noire.
    pub fn render<'n>(&self, canvas: &mut FrameBuffer, names: impl Iterator<Item = &'n str>) {
        // Date et heure a droite ("12 NOV 16:47"), ou "HEURE ?". Pas de jour de
        // la semaine : avec, l'en-tete se colle a "DOCPHONE" (il reste dans
        // l'Horloge).
        let mut header = HeapString::<24>::new();
        match clock::now() {
            Some(now) => {
                let _ = core::fmt::write(
                    &mut header,
                    format_args!(
                        "{:02} {} {:02}:{:02}",
                        now.day,
                        now.month_short(),
                        now.hour,
                        now.minute
                    ),
                );
            }
            None => {
                let _ = header.push_str("HEURE ?");
            }
        }
        flap::draw_header(canvas, "DocPhone", Some(header.as_str()));

        for (index, name) in names.enumerate() {
            let top = LIST_TOP + index as i32 * ROW_PITCH;
            if index == self.selected {
                draw_arrow(canvas, ARROW_LEFT, top + SMALL.tile_height as i32 / 2);
            }
            flap::draw_text(canvas, &SMALL, name, LIST_LEFT, top);
        }

        // Bas de l'ecran : un reset anormal (brownout, watchdog...) passe avant
        // le rappel de reglage de l'heure.
        let reason = system::reset_reason();
        if system::reset_is_abnormal(reason) {
            let mut warning = HeapString::<48>::new();
            let _ = core::fmt::write(
                &mut warning,
                format_args!("Dernier reset : {}", system::reset_reason_label(reason)),
            );
            canvas.text_centered(&warning, 455);
        } else if clock::now().is_none() {
            canvas.text_centered("Regler l'heure : TIME AAAA-MM-JJ HH:MM", 455);
        }
    }

    pub fn handle_input<'n>(&mut self, input: Input, names: impl Iterator<Item = &'n str>) -> AppCommand {
        let count = names.count();
        if count == 0 {
            esp_println::println!("Aucune app enregistree.");
            return AppCommand::Stay;
        }
        match input {
            Input::Key(Key::Up) => {
                self.selected = if self.selected == 0 { count - 1 } else { self.selected - 1 };
                AppCommand::Redraw
            }
            Input::Key(Key::Down) => {
                self.selected = (self.selected + 1) % count;
                AppCommand::Redraw
            }
            Input::Key(Key::Enter) => AppCommand::Open(self.selected),
            Input::Key(Key::Back) => AppCommand::FullRedraw,
            _ => {
                esp_println::println!("Menu : UP, DOWN, ENTER ou BACK.");
                AppCommand::Stay
            }
        }
    }
}
