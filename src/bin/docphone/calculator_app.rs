use esp32_quick::FrameBuffer;
use esp32_quick::input::{Input, Key};
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
    fn name(&self) -> &'static str {
        "Calculatrice"
    }

    fn intro(&self) -> &'static str {
        "expression (ex. 12 + 3), CLEAR ou BACK."
    }

    fn render(&self, canvas: &mut FrameBuffer) {
        canvas.text("Calculatrice", 40, 80);
        canvas.text("Entree :", 40, 150);
        canvas.text(self.expression.as_str(), 40, 190);
        canvas.text(self.result.as_str(), 40, 250);
        canvas.text("Exemple : 12 + 3 | CLEAR | BACK", 40, 440);
    }

    fn handle_input(&mut self, input: Input) -> AppCommand {
        match input {
            Input::Key(Key::Back) => AppCommand::BackToMenu,
            Input::Key(_) => AppCommand::Stay,
            Input::Text("CLEAR") => {
                self.expression.clear();
                self.result.clear();
                AppCommand::Redraw
            }
            Input::Text(input) => {
                self.calculate(input);
                esp_println::println!("{} = {}", self.expression.as_str(), self.result.as_str());
                AppCommand::Redraw
            }
        }
    }
}
