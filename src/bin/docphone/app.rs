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
    /// Redessiner un affichage qui change chaque seconde (chrono, minuteur) :
    /// partiel, avec beaucoup moins de rafraichissements complets.
    LiveRedraw,
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

    /// Appele regulierement (environ 5 fois par seconde), que l'app soit
    /// affichee ou non. Renvoyer `Redraw` quand l'affichage doit changer ; ce
    /// n'est pris en compte que si l'app est affichee. Par defaut : rien.
    fn tick(&mut self) -> AppCommand {
        AppCommand::Stay
    }
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
        launcher.refresh(Refresh::Full);
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
            AppCommand::BackToMenu => self.open(None),
            AppCommand::Open(index) if index < self.apps.len() => self.open(Some(index)),
            other => {
                if let Some(refresh) = Refresh::from_command(&other) {
                    self.refresh(refresh);
                }
            }
        }
    }

    /// A appeler regulierement : laisse le menu et toutes les apps se mettre a
    /// jour (horloge, minuteur...) et redessine l'ecran affiche si besoin.
    pub fn tick(&mut self) {
        // Toutes les apps sont appelees (un minuteur doit pouvoir sonner hors de
        // son app), mais seul l'ecran affiche est redessine.
        let menu_command = self.menu.tick();
        let mut refresh = match self.current {
            None => Refresh::from_command(&menu_command),
            Some(_) => None,
        };

        for (index, app) in self.apps.iter_mut().enumerate() {
            let command = app.tick();
            if self.current == Some(index) {
                refresh = refresh.max(Refresh::from_command(&command));
            }
        }

        if let Some(refresh) = refresh {
            self.refresh(refresh);
        }
    }

    /// Change d'ecran (`None` = menu) avec un rafraichissement complet.
    fn open(&mut self, target: Option<usize>) {
        self.current = target;
        self.refresh(Refresh::Full);
        if let Some(index) = target {
            esp_println::println!("{} : {}", self.apps[index].name(), self.apps[index].intro());
        }
    }

    /// Dessine l'ecran courant et l'envoie a l'ecran.
    fn refresh(&mut self, refresh: Refresh) {
        let Self { apps, menu, display, current } = self;
        let render = |canvas: &mut FrameBuffer| match *current {
            None => menu.render(canvas, apps.iter().map(|app| app.name())),
            Some(index) => apps[index].render(canvas),
        };
        match refresh {
            Refresh::Live => display.draw_live(render),
            Refresh::Normal => display.draw(render),
            Refresh::Full => {
                display.request_full_refresh();
                display.draw(render);
            }
        }
    }
}

/// Type de rafraichissement, du plus leger au plus complet (l'ordre sert a
/// garder le plus fort quand plusieurs demandes arrivent en meme temps).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Refresh {
    Live,
    Normal,
    Full,
}

impl Refresh {
    fn from_command(command: &AppCommand) -> Option<Self> {
        match command {
            AppCommand::LiveRedraw => Some(Refresh::Live),
            AppCommand::Redraw => Some(Refresh::Normal),
            AppCommand::FullRedraw => Some(Refresh::Full),
            AppCommand::Stay | AppCommand::BackToMenu | AppCommand::Open(_) => None,
        }
    }
}
