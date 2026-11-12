use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use esp32_quick::FrameBuffer;
use esp32_quick::clock;
use heapless::String as HeapString;

use super::app::AppCommand;

/// Menu principal. La liste des apps vient du launcher (registre de main.rs) :
/// le menu ne garde que la position de la selection.
pub struct MenuApp {
    selected: usize,
}

impl MenuApp {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    pub fn render<'n>(&self, canvas: &mut FrameBuffer, names: impl Iterator<Item = &'n str>) {
        canvas.text("DocPhone", 40, 80);

        // Date et heure en haut a droite, ou une invite si l'heure n'est pas reglee.
        let mut header = HeapString::<48>::new();
        match clock::now() {
            Some(now) => {
                let _ = core::fmt::write(
                    &mut header,
                    format_args!(
                        "{} {} {} {:02}:{:02}",
                        now.weekday_name(),
                        now.day,
                        now.month_name(),
                        now.hour,
                        now.minute
                    ),
                );
            }
            None => {
                let _ = header.push_str("Heure ? TIME AAAA-MM-JJ HH:MM");
            }
        }
        canvas.text_right(header.as_str(), 40, 80);

        for (index, label) in names.enumerate() {
            let y = 170 + index as i32 * 70;
            if index == self.selected {
                let _ = Rectangle::new(Point::new(32, y - 34), Size::new(360, 48))
                    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                    .draw(canvas);
            }
            canvas.text(label, 50, y);
        }
    }

    pub fn handle_input<'n>(&mut self, input: &str, names: impl Iterator<Item = &'n str>) -> AppCommand {
        let count = names.count();
        if count == 0 {
            esp_println::println!("Aucune app enregistree.");
            return AppCommand::Stay;
        }
        match input {
            "UP" => {
                self.selected = if self.selected == 0 { count - 1 } else { self.selected - 1 };
                AppCommand::Redraw
            }
            "DOWN" => {
                self.selected = (self.selected + 1) % count;
                AppCommand::Redraw
            }
            "ENTER" => AppCommand::Open(self.selected),
            "BACK" => AppCommand::FullRedraw,
            _ => {
                esp_println::println!("Menu : UP, DOWN, ENTER ou BACK.");
                AppCommand::Stay
            }
        }
    }
}
