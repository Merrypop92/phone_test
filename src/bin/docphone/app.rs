//! Systeme d'apps : le trait `App` que chaque app implemente, et le `Launcher`
//! qui affiche le menu, ouvre les apps et leur transmet les saisies.
//!
//! Les apps sont enregistrees a un seul endroit : le tableau `registry` de
//! main.rs. Le menu est construit automatiquement a partir de ce tableau.

use esp32_quick::Epd;

use super::menu_app::MenuApp;

/// Ce que l'app demande au launcher apres avoir traite une saisie.
pub enum AppCommand {
    Stay,
    BackToMenu,
}

pub trait App {
    /// Nom affiche dans le menu.
    fn name(&self) -> &'static str;

    /// Message affiche sur la console a l'ouverture de l'app (commandes principales).
    fn intro(&self) -> &'static str;

    fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]);

    fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand;
}

/// Gere la navigation entre le menu et les apps. Les apps restent en memoire
/// (leurs etats survivent au retour au menu, pas a un redemarrage).
pub struct Launcher<'a> {
    apps: &'a mut [&'a mut dyn App],
    menu: MenuApp,
    /// `None` = menu affiche, `Some(i)` = app numero i ouverte.
    current: Option<usize>,
}

impl<'a> Launcher<'a> {
    pub fn new(apps: &'a mut [&'a mut dyn App]) -> Self {
        Self {
            apps,
            menu: MenuApp::new(),
            current: None,
        }
    }

    pub fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        match self.current {
            None => self.menu.draw(epd, buf, self.apps.iter().map(|app| app.name())),
            Some(index) => self.apps[index].draw(epd, buf),
        }
    }

    pub fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) {
        match self.current {
            None => {
                let names = self.apps.iter().map(|app| app.name());
                if let Some(index) = self.menu.handle_input(input, epd, buf, names) {
                    self.open(Some(index), epd, buf);
                }
            }
            Some(index) => {
                if let AppCommand::BackToMenu = self.apps[index].handle_input(input, epd, buf) {
                    self.open(None, epd, buf);
                }
            }
        }
    }

    /// Apres un reglage de l'heure : redessine le menu s'il est affiche
    /// (c'est le seul ecran qui montre l'heure pour l'instant).
    pub fn on_time_changed(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        if self.current.is_none() {
            self.draw(epd, buf);
        }
    }

    /// Change d'ecran (`None` = menu) avec un rafraichissement complet.
    fn open(&mut self, target: Option<usize>, epd: &mut Epd, buf: &mut [u8]) {
        self.current = target;
        epd.request_full_refresh();
        self.draw(epd, buf);
        if let Some(index) = target {
            esp_println::println!("{} : {}", self.apps[index].name(), self.apps[index].intro());
        }
    }
}
