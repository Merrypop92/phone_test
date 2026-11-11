use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp32_quick::{Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, clear_full_buffer};

use super::app::{App, AppCommand, MenuController};

pub struct MenuApp {
    pub controller: MenuController,
}

impl MenuApp {
    pub fn new() -> Self {
        Self {
            controller: MenuController::new(),
        }
    }
}

fn draw_menu(epd: &mut Epd, buf: &mut [u8], selected: usize, items: [&str; 3]) {
    clear_full_buffer(buf);
    {
        let mut target = FrameBuffer {
            buf,
            width: PX_WIDTH,
            height: PX_HEIGHT,
        };
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let _ = Text::new("DocPhone", Point::new(40, 80), style).draw(&mut target);

        for (index, label) in items.iter().enumerate() {
            let y = 170 + index as i32 * 70;
            if index == selected {
                let _ = Rectangle::new(Point::new(32, y - 34), Size::new(360, 48))
                    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                    .draw(&mut target);
            }
            let _ = Text::new(label, Point::new(50, y), style).draw(&mut target);
        }
    }
    epd.whitescreen_all(buf);
}

impl App for MenuApp {
    fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        draw_menu(epd, buf, self.controller.selected, self.controller.items);
    }

    fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand {
        match input {
            "UP" => {
                self.controller.move_up();
                self.draw(epd, buf);
                AppCommand::Stay
            }
            "DOWN" => {
                self.controller.move_down();
                self.draw(epd, buf);
                AppCommand::Stay
            }
            "ENTER" => self.controller.selected_app(),
            "BACK" => {
                self.draw(epd, buf);
                AppCommand::Stay
            }
            _ => {
                esp_println::println!("Menu : UP, DOWN, ENTER ou BACK.");
                AppCommand::Stay
            }
        }
    }
}
