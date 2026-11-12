use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp32_quick::clock;
use esp32_quick::{Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, clear_full_buffer};
use heapless::String as HeapString;

/// Menu principal. La liste des apps vient du launcher (registre de main.rs) :
/// le menu ne garde que la position de la selection.
pub struct MenuApp {
    selected: usize,
}

impl MenuApp {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    pub fn draw<'n>(&mut self, epd: &mut Epd, buf: &mut [u8], names: impl Iterator<Item = &'n str>) {
        clear_full_buffer(buf);
        {
            let mut target = FrameBuffer {
                buf,
                width: PX_WIDTH,
                height: PX_HEIGHT,
            };
            let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
            let _ = Text::new("DocPhone", Point::new(40, 80), style).draw(&mut target);

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
            let header_x = PX_WIDTH as i32 - 40 - header.len() as i32 * 10;
            let _ = Text::new(header.as_str(), Point::new(header_x, 80), style).draw(&mut target);

            for (index, label) in names.enumerate() {
                let y = 170 + index as i32 * 70;
                if index == self.selected {
                    let _ = Rectangle::new(Point::new(32, y - 34), Size::new(360, 48))
                        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                        .draw(&mut target);
                }
                let _ = Text::new(label, Point::new(50, y), style).draw(&mut target);
            }
        }
        epd.show(buf);
    }

    /// Traite une saisie. Renvoie `Some(i)` si l'app numero i doit etre ouverte.
    pub fn handle_input<'n>(
        &mut self,
        input: &str,
        epd: &mut Epd,
        buf: &mut [u8],
        names: impl Iterator<Item = &'n str> + Clone,
    ) -> Option<usize> {
        let count = names.clone().count();
        if count == 0 {
            esp_println::println!("Aucune app enregistree.");
            return None;
        }
        match input {
            "UP" => {
                self.selected = if self.selected == 0 { count - 1 } else { self.selected - 1 };
                self.draw(epd, buf, names);
                None
            }
            "DOWN" => {
                self.selected = (self.selected + 1) % count;
                self.draw(epd, buf, names);
                None
            }
            "ENTER" => Some(self.selected),
            "BACK" => {
                epd.request_full_refresh();
                self.draw(epd, buf, names);
                None
            }
            _ => {
                esp_println::println!("Menu : UP, DOWN, ENTER ou BACK.");
                None
            }
        }
    }
}
