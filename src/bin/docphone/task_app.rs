use esp32_quick::FrameBuffer;
use esp32_quick::display::flap::{self, SCREEN_MARGIN as MARGIN, SMALL};
use esp32_quick::input::{Input, Key};
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

    fn set_done(&mut self, argument: &str, done: bool) -> AppCommand {
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
        AppCommand::Redraw
    }
}

/// Mise en page du tableau : 8 lignes de palettes (44 px) sous l'en-tete.
const LIST_TOP: i32 = 80;
const ROW_PITCH: i32 = 49;

/// Ligne du tableau pour une tache : "1 [X] ! TITRE", titre coupe avec "..."
/// s'il depasse `max_chars` caracteres au total.
fn task_row(index: usize, task: &Task, max_chars: usize) -> HeapString<80> {
    let mut row = HeapString::<80>::new();
    let _ = core::fmt::write(
        &mut row,
        format_args!(
            "{} [{}] {}",
            index + 1,
            if task.done { "X" } else { " " },
            if task.urgent { "! " } else { "" }
        ),
    );
    let room = max_chars.saturating_sub(row.chars().count());
    let title_len = task.title.chars().count();
    if title_len <= room {
        let _ = row.push_str(task.title.as_str());
    } else {
        let mut cut = HeapString::<80>::new();
        for c in task.title.chars().take(room.saturating_sub(3)) {
            let _ = cut.push(c);
        }
        // Pas de palette vide avant "..." si la coupe tombe sur un espace.
        let _ = row.push_str(cut.trim_end());
        let _ = row.push_str("...");
    }
    row
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

impl App for TaskApp {
    fn name(&self) -> &'static str {
        "Taches"
    }

    fn intro(&self) -> &'static str {
        "ADD [!] <texte>, DONE <n>, CLEAR... Tape HELP pour toutes les commandes."
    }

    /// Tableau des departs : titre et compteur en petites palettes, puis une
    /// ligne par tache ("1 [X] ! TITRE"), titre coupe si trop long.
    fn render(&self, canvas: &mut FrameBuffer) {
        let done_count = self.done_count();
        let mut header = HeapString::<32>::new();
        let _ = core::fmt::write(
            &mut header,
            format_args!(
                "Taches {}/{} - {} faite{}",
                self.tasks.len(),
                MAX_TASKS,
                done_count,
                if done_count > 1 { "s" } else { "" }
            ),
        );
        flap::draw_header(canvas, header.as_str(), None);

        if self.tasks.is_empty() {
            flap::draw_text(canvas, &SMALL, "Aucune tache", MARGIN, LIST_TOP);
            canvas.text("ADD <texte> pour ajouter une tache, HELP pour l'aide.", MARGIN, LIST_TOP + 90);
            return;
        }

        let max_chars = SMALL.max_chars(canvas.width as i32 - 2 * MARGIN);
        for (index, task) in self.tasks.iter().enumerate() {
            let row = task_row(index, task, max_chars);
            flap::draw_text(canvas, &SMALL, row.as_str(), MARGIN, LIST_TOP + index as i32 * ROW_PITCH);
        }
    }

    fn handle_input(&mut self, input: Input) -> AppCommand {
        // Confirmation d'un CLEAR : OUI valide, toute autre saisie annule
        // puis est traitee normalement.
        if self.clear_pending {
            self.clear_pending = false;
            if input == Input::Text("OUI") {
                let before = self.tasks.len();
                self.tasks.retain(|task| !task.done);
                esp_println::println!("{} tache(s) supprimee(s).", before - self.tasks.len());
                return AppCommand::Redraw;
            }
            esp_println::println!("CLEAR annule.");
        }

        let input = match input {
            Input::Key(Key::Back) => return AppCommand::BackToMenu,
            // Les autres touches ne servent pas encore ici (plus tard : selection d'une tache).
            Input::Key(_) => return AppCommand::Stay,
            Input::Text(text) => text,
        };

        if input == "HELP" {
            print_help();
            return AppCommand::Stay;
        }

        if input == "LIST" {
            esp_println::println!("{} tache(s).", self.tasks.len());
            return AppCommand::FullRedraw;
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
                return AppCommand::Redraw;
            }
            return AppCommand::Stay;
        }

        if let Some(argument) = input.strip_prefix("DONE ") {
            return self.set_done(argument, true);
        }

        if let Some(argument) = input.strip_prefix("UNDO ") {
            return self.set_done(argument, false);
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
            return AppCommand::Redraw;
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
            return AppCommand::Redraw;
        }

        if let Some(argument) = input.strip_prefix("DELETE ") {
            let Some(index) = self.parse_index(argument) else {
                esp_println::println!("Usage : DELETE <numero de tache>");
                return AppCommand::Stay;
            };
            self.tasks.remove(index);
            esp_println::println!("Tache {} supprimee.", index + 1);
            return AppCommand::Redraw;
        }

        esp_println::println!("Commande inconnue. Tape HELP pour la liste des commandes.");
        AppCommand::Stay
    }
}
