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

    /// Convertit un numero affiche (1..=len) en index du tableau.
    fn parse_index(&self, text: &str) -> Option<usize> {
        match text.trim().parse::<usize>() {
            Ok(n) if n > 0 && n <= self.tasks.len() => Some(n - 1),
            _ => None,
        }
    }
}

/// Verifie qu'un titre n'est pas vide et tient dans MAX_TITLE_BYTES.
fn parse_title(text: &str) -> Option<HeapString<MAX_TITLE_BYTES>> {
    let text = text.trim();
    if text.is_empty() {
        esp_println::println!("Le texte ne peut pas etre vide.");
        return None;
    }
    let mut title = HeapString::new();
    if title.push_str(text).is_err() {
        esp_println::println!("Titre trop long (max {} octets).", MAX_TITLE_BYTES);
        return None;
    }
    Some(title)
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

        let _ = Text::new(
            "ADD | EDIT <n> | DONE/UNDO <n> | DELETE <n> | BACK",
            Point::new(40, 465),
            style,
        )
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

        if let Some(argument) = input.strip_prefix("ADD ") {
            if self.tasks.is_full() {
                esp_println::println!("Liste pleine ({} taches max).", MAX_TASKS);
                return AppCommand::Stay;
            }
            let Some(title) = parse_title(argument) else {
                return AppCommand::Stay;
            };
            if self.tasks.push(Task { title, done: false }).is_ok() {
                esp_println::println!("Tache ajoutee ({}).", self.tasks.len());
                self.draw(epd, buf);
            }
            return AppCommand::Stay;
        }

        if let Some(argument) = input.strip_prefix("DONE ") {
            return self.set_done(argument, true, epd, buf);
        }

        if let Some(argument) = input.strip_prefix("UNDO ") {
            return self.set_done(argument, false, epd, buf);
        }

        if let Some(arguments) = input.strip_prefix("EDIT ") {
            let index = arguments
                .split_once(' ')
                .and_then(|(number, _)| self.parse_index(number));
            let Some(index) = index else {
                esp_println::println!("Usage : EDIT <numero> <nouveau texte>");
                return AppCommand::Stay;
            };
            let text = arguments.split_once(' ').map_or("", |(_, text)| text);
            let Some(title) = parse_title(text) else {
                return AppCommand::Stay;
            };
            self.tasks[index].title = title;
            esp_println::println!("Tache {} modifiee.", index + 1);
            self.draw(epd, buf);
            return AppCommand::Stay;
        }

        if let Some(argument) = input.strip_prefix("DELETE ") {
            let Some(index) = self.parse_index(argument) else {
                esp_println::println!("Usage : DELETE <numero de tache>");
                return AppCommand::Stay;
            };
            self.tasks.remove(index);
            esp_println::println!("Tache {} supprimee.", index + 1);
            self.draw(epd, buf);
            return AppCommand::Stay;
        }

        esp_println::println!(
            "Commande inconnue. Utilise ADD, EDIT, DONE, UNDO, DELETE, LIST ou BACK."
        );
        AppCommand::Stay
    }
}

impl TaskApp {
    fn set_done(&mut self, argument: &str, done: bool, epd: &mut Epd, buf: &mut [u8]) -> AppCommand {
        let Some(index) = self.parse_index(argument) else {
            esp_println::println!("Usage : {} <numero de tache>", if done { "DONE" } else { "UNDO" });
            return AppCommand::Stay;
        };
        self.tasks[index].done = done;
        esp_println::println!(
            "Tache {} {}.",
            index + 1,
            if done { "terminee" } else { "a faire" }
        );
        self.draw(epd, buf);
        AppCommand::Stay
    }
}
