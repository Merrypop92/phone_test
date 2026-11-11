use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use esp32_quick::{Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, clear_full_buffer};
use heapless::String as HeapString;

use super::app::{App, AppCommand};

pub struct CalculatorApp {
    expression: HeapString<64>,
    result: HeapString<64>,
}

impl CalculatorApp {
    pub fn new() -> Self {
        Self {
            expression: HeapString::new(),
            result: HeapString::new(),
        }
    }

    fn calculate(&mut self, input: &str) {
        self.expression.clear();
        let _ = self.expression.push_str(input);
        self.result.clear();

        let mut tokens = input.split_whitespace();
        let left = tokens.next();
        let operator = tokens.next();
        let right = tokens.next();

        if tokens.next().is_some() || left.is_none() || right.is_none() {
            let _ = self.result.push_str("Format invalide. Exemple : 12 + 3");
            return;
        }

        let left = left.and_then(|value| value.parse::<i32>().ok());
        let right = right.and_then(|value| value.parse::<i32>().ok());
        let (Some(left), Some(right)) = (left, right) else {
            let _ = self.result.push_str("Nombres invalides");
            return;
        };

        let value = match operator {
            Some("+") => left.checked_add(right),
            Some("-") => left.checked_sub(right),
            Some("*") => left.checked_mul(right),
            Some("/") if right != 0 => left.checked_div(right),
            Some("/") => {
                let _ = self.result.push_str("Division par zero");
                return;
            }
            _ => {
                let _ = self.result.push_str("Operateur invalide. Utilise + - * /");
                return;
            }
        };

        match value {
            Some(value) => {
                let _ = core::fmt::write(&mut self.result, format_args!("Resultat : {value}"));
            }
            None => {
                let _ = self.result.push_str("Depassement de capacite");
            }
        }
    }
}

impl App for CalculatorApp {
    fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        clear_full_buffer(buf);
        {
            let mut target = FrameBuffer {
                buf,
                width: PX_WIDTH,
                height: PX_HEIGHT,
            };
            let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
            let _ = Text::new("Calculatrice", Point::new(40, 80), style).draw(&mut target);
            let _ = Text::new("Entree :", Point::new(40, 150), style).draw(&mut target);
            let _ = Text::new(self.expression.as_str(), Point::new(40, 190), style).draw(&mut target);
            let _ = Text::new(self.result.as_str(), Point::new(40, 250), style).draw(&mut target);
            let _ = Text::new("Exemple : 12 + 3 | CLEAR | BACK", Point::new(40, 440), style)
                .draw(&mut target);
        }
        epd.whitescreen_all(buf);
    }

    fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand {
        match input {
            "BACK" => AppCommand::BackToMenu,
            "CLEAR" => {
                self.expression.clear();
                self.result.clear();
                self.draw(epd, buf);
                AppCommand::Stay
            }
            _ => {
                self.calculate(input);
                self.draw(epd, buf);
                esp_println::println!("{} = {}", self.expression.as_str(), self.result.as_str());
                AppCommand::Stay
            }
        }
    }
}
