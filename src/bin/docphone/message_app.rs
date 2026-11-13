use esp32_quick::FrameBuffer;
use esp32_quick::display::flap::{self, SCREEN_MARGIN as MARGIN, SMALL};
use esp32_quick::input::{Input, Key};
use heapless::String as HeapString;

use super::app::{App, AppCommand};

/// Mise en page.
const MESSAGE_TOP: i32 = 100;
/// Pas vertical des lignes de petites palettes.
const LINE_PITCH: i32 = 56;
/// Un message de 64 octets tient en 3 lignes de 26 caracteres ; 4 par
/// securite, la coupure aux espaces pouvant laisser des fins de ligne vides.
const MAX_LINES: usize = 4;

pub struct MessageApp {
    pub last_message: HeapString<64>,
}

impl MessageApp {
    pub fn new() -> Self {
        Self {
            last_message: HeapString::new(),
        }
    }
}

impl App for MessageApp {
    fn name(&self) -> &'static str {
        "Messages"
    }

    fn intro(&self) -> &'static str {
        "texte libre ou BACK."
    }

    /// En-tete en petites palettes, puis le dernier message en petites
    /// palettes, coupe sur plusieurs lignes.
    fn render(&self, canvas: &mut FrameBuffer) {
        flap::draw_header(canvas, "Messages", None);

        if self.last_message.is_empty() {
            flap::draw_text(canvas, &SMALL, "Aucun message", MARGIN, MESSAGE_TOP);
        } else {
            let max_chars = SMALL.max_chars(canvas.width as i32 - 2 * MARGIN);
            let message = self.last_message.as_str();
            flap::draw_text_wrapped(canvas, &SMALL, message, MARGIN, MESSAGE_TOP, max_chars, LINE_PITCH, MAX_LINES);
        }

        canvas.text_centered("Tape un texte puis Entree   |   BACK", 465);
    }

    fn handle_input(&mut self, input: Input) -> AppCommand {
        let input = match input {
            Input::Key(Key::Back) => return AppCommand::BackToMenu,
            Input::Key(_) => return AppCommand::Stay,
            Input::Text(text) => text,
        };

        self.last_message.clear();
        let _ = self.last_message.push_str(input);
        esp_println::println!("Affichage e-paper: {input}");
        AppCommand::Redraw
    }
}
