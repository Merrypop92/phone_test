use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use esp32_quick::{Epd, FrameBuffer, PX_HEIGHT, PX_WIDTH, clear_full_buffer};
use heapless::{String as HeapString, Vec as HeapVec};

use super::app::{App, AppCommand};

pub const MAX_TASKS: usize = 8;
pub const MAX_TITLE_BYTES: usize = 48;

pub struct Task {
    pub title: HeapString<MAX_TITLE_BYTES>,
    pub done: bool,
    pub urgent: bool,
}

pub struct TaskApp {
    pub tasks: HeapVec<Task, MAX_TASKS>,
    /// Vrai entre un `CLEAR` et sa confirmation par `OUI`.
    clear_pending: bool,
}

impl TaskApp {
    pub fn new() -> Self {
        Self {
            tasks: HeapVec::new(),
            clear_pending: false,
        }
    }

    /// Convertit un numero affiche (1..=len) en index du tableau.
    fn parse_index(&self, text: &str) -> Option<usize> {
        match text.trim().parse::<usize>() {
            Ok(n) if n > 0 && n <= self.tasks.len() => Some(n - 1),
            _ => None,
        }
    }

    fn done_count(&self) -> usize {
        self.tasks.iter().filter(|task| task.done).count()
    }

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

fn print_help() {
    esp_println::println!("Commandes Taches :");
    esp_println::println!("  ADD <texte>       ajouter une tache");
    esp_println::println!("  ADD ! <texte>     ajouter une tache urgente (marquee !)");
    esp_println::println!("  EDIT <n> <texte>  modifier le titre de la tache n");
    esp_println::println!("  DONE <n>          marquer la tache n terminee");
    esp_println::println!("  UNDO <n>          remettre la tache n a faire");
    esp_println::println!("  URGENT <n>        marquer / demarquer la tache n comme urgente");
    esp_println::println!("  DELETE <n>        supprimer la tache n");
    esp_println::println!("  CLEAR             supprimer les taches terminees (demande OUI)");
    esp_println::println!("  LIST              reafficher la liste (rafraichissement complet)");
    esp_println::println!("  HELP              afficher cette aide");
    esp_println::println!("  BACK              revenir au menu");
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

        let done_count = tasks.iter().filter(|task| task.done).count();
        let mut header = HeapString::<48>::new();
        let _ = core::fmt::write(
            &mut header,
            format_args!(
                "Taches {}/{} - {} faite{} (RAM)",
                tasks.len(),
                MAX_TASKS,
                done_count,
                if done_count > 1 { "s" } else { "" }
            ),
        );
        let _ = Text::new(header.as_str(), Point::new(40, 70), style).draw(&mut target);

        if tasks.is_empty() {
            let _ = Text::new("Aucune tache. ADD <texte> pour commencer.", Point::new(40, 140), style)
                .draw(&mut target);
        }

        for (index, task) in tasks.iter().enumerate() {
            let mut label = HeapString::<64>::new();
            let _ = core::fmt::write(
                &mut label,
                format_args!(
                    "{}. [{}] {}{}",
                    index + 1,
                    if task.done { "x" } else { " " },
                    if task.urgent { "! " } else { "" },
                    task.title.as_str()
                ),
            );
            let y = 140 + index as i32 * 40;
            let _ = Text::new(label.as_str(), Point::new(40, y), style).draw(&mut target);
        }

        let _ = Text::new(
            "ADD [!] | DONE/UNDO | URGENT | CLEAR | HELP",
            Point::new(40, 465),
            style,
        )
        .draw(&mut target);
    }
    epd.show(buf);
}

impl App for TaskApp {
    fn name(&self) -> &'static str {
        "Taches"
    }

    fn intro(&self) -> &'static str {
        "ADD [!] <texte>, DONE <n>, CLEAR... Tape HELP pour toutes les commandes."
    }

    fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        draw_tasks(epd, buf, &self.tasks);
    }

    fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand {
        // Confirmation d'un CLEAR : OUI valide, toute autre saisie annule
        // puis est traitee normalement.
        if self.clear_pending {
            self.clear_pending = false;
            if input == "OUI" {
                let before = self.tasks.len();
                self.tasks.retain(|task| !task.done);
                esp_println::println!("{} tache(s) supprimee(s).", before - self.tasks.len());
                self.draw(epd, buf);
                return AppCommand::Stay;
            }
            esp_println::println!("CLEAR annule.");
        }

        if input == "BACK" {
            return AppCommand::BackToMenu;
        }

        if input == "HELP" {
            print_help();
            return AppCommand::Stay;
        }

        if input == "LIST" {
            epd.request_full_refresh();
            self.draw(epd, buf);
            esp_println::println!("{} tache(s).", self.tasks.len());
            return AppCommand::Stay;
        }

        if input == "CLEAR" {
            let count = self.done_count();
            if count == 0 {
                esp_println::println!("Aucune tache terminee a supprimer.");
                return AppCommand::Stay;
            }
            esp_println::println!(
                "Supprimer {} tache(s) terminee(s) ? Tape OUI pour confirmer, autre chose pour annuler.",
                count
            );
            self.clear_pending = true;
            return AppCommand::Stay;
        }

        if let Some(argument) = input.strip_prefix("ADD ") {
            if self.tasks.is_full() {
                esp_println::println!("Liste pleine ({} taches max).", MAX_TASKS);
                return AppCommand::Stay;
            }
            let argument = argument.trim_start();
            let (urgent, text) = match argument.strip_prefix("! ") {
                Some(text) => (true, text),
                None => (false, argument),
            };
            let Some(title) = parse_title(text) else {
                return AppCommand::Stay;
            };
            if self.tasks.push(Task { title, done: false, urgent }).is_ok() {
                esp_println::println!(
                    "Tache{} ajoutee ({}).",
                    if urgent { " urgente" } else { "" },
                    self.tasks.len()
                );
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

        if let Some(argument) = input.strip_prefix("URGENT ") {
            let Some(index) = self.parse_index(argument) else {
                esp_println::println!("Usage : URGENT <numero de tache>");
                return AppCommand::Stay;
            };
            let task = &mut self.tasks[index];
            task.urgent = !task.urgent;
            esp_println::println!(
                "Tache {} {}.",
                index + 1,
                if task.urgent { "marquee urgente" } else { "n'est plus urgente" }
            );
            self.draw(epd, buf);
            return AppCommand::Stay;
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

        esp_println::println!("Commande inconnue. Tape HELP pour la liste des commandes.");
        AppCommand::Stay
    }
}
