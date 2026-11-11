use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use esp32_quick::{Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, clear_full_buffer};
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

fn draw_message(epd: &mut Epd, buf: &mut [u8], message: &str) {
    clear_full_buffer(buf);
    {
        let mut target = FrameBuffer {
            buf,
            width: PX_WIDTH,
            height: PX_HEIGHT,
        };
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let _ = Text::new("Message recu :", Point::new(40, 100), style).draw(&mut target);
        let _ = Text::new(message, Point::new(40, 160), style).draw(&mut target);
    }
    epd.whitescreen_all(buf);
}

impl App for MessageApp {
    fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        if self.last_message.is_empty() {
            draw_message(epd, buf, "Tape un texte puis Entree");
        } else {
            draw_message(epd, buf, self.last_message.as_str());
        }
    }

    fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand {
        if input == "BACK" {
            return AppCommand::BackToMenu;
        }

        self.last_message.clear();
        let _ = self.last_message.push_str(input);
        self.draw(epd, buf);
        esp_println::println!("Affichage e-paper: {input}");
        AppCommand::Stay
    }
}
