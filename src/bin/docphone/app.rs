//! Systeme d'apps : le trait `App` que chaque app implemente, et le `Launcher`
//! qui affiche le menu, ouvre les apps et leur transmet les saisies.
//!
//! Les apps sont enregistrees a un seul endroit : le tableau `registry` de
//! main.rs. Le menu est construit automatiquement a partir de ce tableau.
//!
//! Les apps ne touchent jamais l'ecran directement : elles dessinent dans
//! `render()`, et disent au launcher (via `AppCommand`) s'il faut redessiner.

use esp32_quick::FrameBuffer;
use esp32_quick::display::Display;
use esp32_quick::input::Input;

use super::menu_app::MenuApp;

/// Ce que l'app (ou le menu) demande au launcher apres une saisie.
pub enum AppCommand {
    /// Rien a redessiner.
    Stay,
    /// Redessiner l'ecran (rafraichissement partiel, rapide).
    Redraw,
    /// Redessiner avec un rafraichissement complet (efface les traces).
    FullRedraw,
    /// Revenir au menu.
    BackToMenu,
    /// Ouvrir l'app numero i du registre (utilise par le menu).
    Open(usize),
}

pub trait App {
    /// Nom affiche dans le menu.
    fn name(&self) -> &'static str;

    /// Message affiche sur la console a l'ouverture de l'app (commandes principales).
    fn intro(&self) -> &'static str;

    /// Dessine l'ecran de l'app. Le tampon est deja vide (blanc).
    fn render(&self, canvas: &mut FrameBuffer);

    /// Traite une entree (touche ou texte). `Key::Back` doit ramener au menu.
    fn handle_input(&mut self, input: Input) -> AppCommand;
}

/// Gere la navigation entre le menu et les apps, et l'affichage. Les apps
/// restent en memoire (leurs etats survivent au retour au menu, pas a un
/// redemarrage).
pub struct Launcher<'a, 'd> {
    apps: &'a mut [&'a mut dyn App],
    menu: MenuApp,
    display: Display<'d>,
    /// `None` = menu affiche, `Some(i)` = app numero i ouverte.
    current: Option<usize>,
}

impl<'a, 'd> Launcher<'a, 'd> {
    /// Cree le launcher et affiche le menu.
    pub fn new(apps: &'a mut [&'a mut dyn App], display: Display<'d>) -> Self {
        let mut launcher = Self {
            apps,
            menu: MenuApp::new(),
            display,
            current: None,
        };
        launcher.redraw();
        launcher
    }

    pub fn handle_input(&mut self, input: Input) {
        let command = match self.current {
            None => {
                let names = self.apps.iter().map(|app| app.name());
                self.menu.handle_input(input, names)
            }
            Some(index) => self.apps[index].handle_input(input),
        };

        match command {
            AppCommand::Stay => {}
            AppCommand::Redraw => self.redraw(),
            AppCommand::FullRedraw => {
                self.display.request_full_refresh();
                self.redraw();
            }
            AppCommand::BackToMenu => self.open(None),
            AppCommand::Open(index) if index < self.apps.len() => self.open(Some(index)),
            AppCommand::Open(_) => {}
        }
    }

    /// Apres un reglage de l'heure : redessine le menu s'il est affiche
    /// (c'est le seul ecran qui montre l'heure pour l'instant).
    pub fn on_time_changed(&mut self) {
        if self.current.is_none() {
            self.redraw();
        }
    }

    /// Change d'ecran (`None` = menu) avec un rafraichissement complet.
    fn open(&mut self, target: Option<usize>) {
        self.current = target;
        self.display.request_full_refresh();
        self.redraw();
        if let Some(index) = target {
            esp_println::println!("{} : {}", self.apps[index].name(), self.apps[index].intro());
        }
    }

    /// Dessine l'ecran courant et l'envoie a l'ecran.
    fn redraw(&mut self) {
        let Self { apps, menu, display, current } = self;
        display.draw(|canvas| match *current {
            None => menu.render(canvas, apps.iter().map(|app| app.name())),
            Some(index) => apps[index].render(canvas),
        });
    }
}
