use esp32_quick::FrameBuffer;
use esp32_quick::display::flap::{self, LARGE, SCREEN_MARGIN as MARGIN, SMALL};
use esp32_quick::input::{Input, Key};
use heapless::String as HeapString;

use super::app::{App, AppCommand};

/// Mise en page.
const EXPRESSION_TOP: i32 = 90;
/// Pas vertical des lignes de petites palettes.
const LINE_PITCH: i32 = 52;
/// Centre vertical du resultat en grandes palettes.
const RESULT_CENTER_Y: i32 = 300;

pub struct CalculatorApp {
    expression: HeapString<64>,
    /// Texte du resultat ou de l'erreur (aussi affiche sur la console).
    result: HeapString<64>,
    /// Valeur du dernier calcul reussi (affichee en grandes palettes).
    value: Option<i32>,
}

impl CalculatorApp {
    pub fn new() -> Self {
        Self {
            expression: HeapString::new(),
            result: HeapString::new(),
            value: None,
        }
    }

    fn calculate(&mut self, input: &str) {
        self.expression.clear();
        let _ = self.expression.push_str(input);
        self.result.clear();
        self.value = None;

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
                self.value = Some(value);
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

    /// En-tete et expression en petites palettes, resultat en grandes palettes
    /// s'il tient sur l'ecran (sinon en petites), erreurs en petites palettes.
    fn render(&self, canvas: &mut FrameBuffer) {
        flap::draw_header(canvas, "Calculatrice", None);

        let width = canvas.width as i32 - 2 * MARGIN;
        let max_chars = SMALL.max_chars(width);

        if self.expression.is_empty() {
            flap::draw_text(canvas, &SMALL, "Tape un calcul", MARGIN, EXPRESSION_TOP);
        } else {
            let expression = self.expression.as_str();
            flap::draw_text_wrapped(canvas, &SMALL, expression, MARGIN, EXPRESSION_TOP, max_chars, LINE_PITCH, 2);
        }

        match self.value {
            Some(value) => {
                let mut text = HeapString::<16>::new();
                let _ = core::fmt::write(&mut text, format_args!("{value}"));
                if LARGE.text_width(text.as_str()) <= width {
                    flap::draw_row(canvas, text.as_str(), RESULT_CENTER_Y);
                } else {
                    let mut line = HeapString::<20>::new();
                    let _ = core::fmt::write(&mut line, format_args!("= {value}"));
                    flap::draw_text_centered(canvas, &SMALL, line.as_str(), RESULT_CENTER_Y - SMALL.tile_height as i32 / 2);
                }
            }
            None if !self.result.is_empty() => {
                let top = EXPRESSION_TOP + 2 * LINE_PITCH + 20;
                flap::draw_text_wrapped(canvas, &SMALL, self.result.as_str(), MARGIN, top, max_chars, LINE_PITCH, 3);
            }
            None => {}
        }

        canvas.text_centered("Exemple : 12 + 3   |   CLEAR   |   BACK", 465);
    }

    fn handle_input(&mut self, input: Input) -> AppCommand {
        match input {
            Input::Key(Key::Back) => AppCommand::BackToMenu,
            Input::Key(_) => AppCommand::Stay,
            Input::Text("CLEAR") => {
                self.expression.clear();
                self.result.clear();
                self.value = None;
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
