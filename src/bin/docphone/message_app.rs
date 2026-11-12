use esp32_quick::FrameBuffer;
use heapless::String as HeapString;

use super::app::{App, AppCommand};

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

    fn render(&self, canvas: &mut FrameBuffer) {
        canvas.text("Message recu :", 40, 100);
        if self.last_message.is_empty() {
            canvas.text("Tape un texte puis Entree", 40, 160);
        } else {
            canvas.text(self.last_message.as_str(), 40, 160);
        }
    }

    fn handle_input(&mut self, input: &str) -> AppCommand {
        if input == "BACK" {
            return AppCommand::BackToMenu;
        }

        self.last_message.clear();
        let _ = self.last_message.push_str(input);
        esp_println::println!("Affichage e-paper: {input}");
        AppCommand::Redraw
    }
}
