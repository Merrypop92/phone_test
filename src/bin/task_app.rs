use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use esp32_quick::{Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, clear_full_buffer};
use heapless::{String as HeapString, Vec as HeapVec};

use super::app::{App, AppCommand, Task, MAX_TASKS, MAX_TITLE_BYTES};

pub struct TaskApp {
    pub tasks: HeapVec<Task, MAX_TASKS>,
}

impl TaskApp {
    pub fn new() -> Self {
        Self { tasks: HeapVec::new() }
    }
}

fn draw_tasks(epd: &mut Epd, buf: &mut [u8], tasks: &HeapVec<Task, MAX_TASKS>) {
    clear_full_buffer(buf);
    {
        let mut target = FrameBuffer {
            buf,
            width: PX_WIDTH,
            height: PX_HEIGHT,
        };
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let _ = Text::new("Taches (RAM)", Point::new(40, 70), style).draw(&mut target);

        if tasks.is_empty() {
            let _ = Text::new("Aucune tache. ADD <texte> pour commencer.", Point::new(40, 140), style)
                .draw(&mut target);
        }

        for (index, task) in tasks.iter().enumerate() {
            let mut label = HeapString::<64>::new();
            let _ = core::fmt::write(
                &mut label,
                format_args!(
                    "{}. [{}] {}",
                    index + 1,
                    if task.done { "x" } else { " " },
                    task.title.as_str()
                ),
            );
            let y = 140 + index as i32 * 40;
            let _ = Text::new(label.as_str(), Point::new(40, y), style).draw(&mut target);
        }

        let _ = Text::new("EDIT <n> <texte> | DELETE <n>", Point::new(40, 465), style)
            .draw(&mut target);
    }
    epd.whitescreen_all(buf);
}

impl App for TaskApp {
    fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        draw_tasks(epd, buf, &self.tasks);
    }

    fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand {
        if input == "BACK" {
            return AppCommand::BackToMenu;
        }

        if input == "LIST" {
            self.draw(epd, buf);
            esp_println::println!("{} tache(s).", self.tasks.len());
            return AppCommand::Stay;
        }

        if input.starts_with("ADD ") {
            let title = input[4..].trim();
            if title.is_empty() {
                esp_println::println!("Usage : ADD <texte>");
                return AppCommand::Stay;
            }
            if title.len() > MAX_TITLE_BYTES {
                esp_println::println!("Titre trop long (max {} octets).", MAX_TITLE_BYTES);
                return AppCommand::Stay;
            }
            if self.tasks.is_full() {
                esp_println::println!("Liste pleine ({} taches max).", MAX_TASKS);
                return AppCommand::Stay;
            }

            let mut stored_title = HeapString::<MAX_TITLE_BYTES>::new();
            if stored_title.push_str(title).is_ok()
                && self.tasks.push(Task { title: stored_title, done: false }).is_ok()
            {
                esp_println::println!("Tache ajoutee ({}).", self.tasks.len());
                self.draw(epd, buf);
            }
            return AppCommand::Stay;
        }

        if input.starts_with("DONE ") {
            match input[5..].trim().parse::<usize>() {
                Ok(index) if index > 0 && index <= self.tasks.len() => {
                    self.tasks[index - 1].done = true;
                    esp_println::println!("Tache {} terminee.", index);
                    self.draw(epd, buf);
                    return AppCommand::Stay;
                }
                _ => {
                    esp_println::println!("Usage : DONE <numero de tache>");
                    return AppCommand::Stay;
                }
            }
        }

        if let Some(arguments) = input.strip_prefix("EDIT ") {
            let Some((number, title)) = arguments.split_once(' ') else {
                esp_println::println!("Usage : EDIT <numero> <nouveau texte>");
                return AppCommand::Stay;
            };

            let title = title.trim();
            if title.is_empty() {
                esp_println::println!("Le texte ne peut pas etre vide.");
                return AppCommand::Stay;
            }
            if title.len() > MAX_TITLE_BYTES {
                esp_println::println!("Titre trop long (max {} octets).", MAX_TITLE_BYTES);
                return AppCommand::Stay;
            }

            match number.parse::<usize>() {
                Ok(index) if index > 0 && index <= self.tasks.len() => {
                    self.tasks[index - 1].title.clear();
                    let _ = self.tasks[index - 1].title.push_str(title);
                    esp_println::println!("Tache {} modifiee.", index);
                    self.draw(epd, buf);
                    return AppCommand::Stay;
                }
                _ => {
                    esp_println::println!("Usage : EDIT <numero> <nouveau texte>");
                    return AppCommand::Stay;
                }
            }
        }

        if input.starts_with("DELETE ") {
            match input[7..].trim().parse::<usize>() {
                Ok(index) if index > 0 && index <= self.tasks.len() => {
                    self.tasks.remove(index - 1);
                    esp_println::println!("Tache {} supprimee.", index);
                    self.draw(epd, buf);
                    return AppCommand::Stay;
                }
                _ => {
                    esp_println::println!("Usage : DELETE <numero de tache>");
                    return AppCommand::Stay;
                }
            }
        }

        esp_println::println!("Commande inconnue. Utilise ADD, EDIT, DONE, DELETE, LIST ou BACK.");
        AppCommand::Stay
    }
}
