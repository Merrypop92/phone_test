//! App Horloge : heure, chronometre et minuteur, affiches facon panneau de gare
//! a palettes (voir esp32_quick::display::flap).
//!
//! L'app memorise le texte affiche en palettes (`shown`, ex. "16:47"). Quand il
//! change, elle affiche d'abord l'image a mi-bascule (haut = nouveau chiffre,
//! bas = chiffre precedent), puis l'image finale au tick suivant.
//! Mises a jour en mode "live" (`LiveRedraw`) :
//! - Horloge : chaque minute ;
//! - Chronometre et minuteur en marche : chaque seconde.
//! STOP fait un rafraichissement complet pour effacer les traces et afficher la
//! valeur exacte (1/10 s pour le chrono).
//!
//! Chronometre et minuteur utilisent le compteur depuis le demarrage : ils
//! fonctionnent meme si l'heure n'est pas reglee, et continuent quand on
//! revient au menu.

use esp32_quick::FrameBuffer;
use esp32_quick::clock::{self, uptime_ms};
use esp32_quick::display::flap;
use esp32_quick::input::{Input, Key};
use heapless::String as HeapString;

use super::app::{App, AppCommand};

/// Centre vertical de la rangee de palettes.
const FLAP_CENTER_Y: i32 = 230;
/// Animation de bascule des palettes (une image intermediaire par changement,
/// voir display::flap). A 1 Hz (chrono, minuteur) cela fait 2 rafraichissements
/// partiels par seconde : ~0,92 s de travail avec le SPI a 10 MHz.
const FLIP_ANIMATION: bool = true;

/// Intervalle de mise a jour du chronometre en marche, en ms. En dessous de
/// 1000, les dixiemes de seconde sont aussi affiches. Minimum realiste : ~500 ms
/// (rafraichissement partiel constructeur : 0,42 s, + ~0,04 s d'envoi SPI a 10 MHz),
/// et le double avec l'animation de bascule.
const STOPWATCH_REFRESH_MS: u64 = 1000;

/// Duree maximale du minuteur, en minutes.
const MAX_TIMER_MINUTES: u64 = 999;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Clock,
    Stopwatch,
    Timer,
}

impl Mode {
    const ALL: [Mode; 3] = [Mode::Clock, Mode::Stopwatch, Mode::Timer];

    fn label(self) -> &'static str {
        match self {
            Mode::Clock => "Horloge",
            Mode::Stopwatch => "Chrono",
            Mode::Timer => "Minuteur",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&mode| mode == self).unwrap_or(0)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    fn previous(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Temps ecoule, qu'on peut mettre en pause : base de chrono et minuteur.
#[derive(Clone, Copy, Default)]
struct Elapsed {
    /// Temps cumule avant la derniere mise en marche.
    accumulated_ms: u64,
    /// Instant (uptime) de la derniere mise en marche, si en marche.
    started_at: Option<u64>,
}

impl Elapsed {
    fn running(&self) -> bool {
        self.started_at.is_some()
    }

    fn get(&self, now: u64) -> u64 {
        self.accumulated_ms + self.started_at.map_or(0, |start| now.saturating_sub(start))
    }

    fn start(&mut self, now: u64) {
        if self.started_at.is_none() {
            self.started_at = Some(now);
        }
    }

    fn stop(&mut self, now: u64) {
        self.accumulated_ms = self.get(now);
        self.started_at = None;
    }
}

struct Timer {
    duration_ms: u64,
    elapsed: Elapsed,
    finished: bool,
}

impl Timer {
    fn remaining(&self, now: u64) -> u64 {
        self.duration_ms.saturating_sub(self.elapsed.get(now))
    }
}

/// Texte d'une rangee de palettes (au plus "100:00.0" ou "TERMINE").
type FlapText = HeapString<16>;

pub struct ClockApp {
    mode: Mode,
    stopwatch: Elapsed,
    timer: Timer,
    /// Texte actuellement affiche en palettes : on ne redessine que quand le
    /// texte a afficher change.
    shown: FlapText,
    /// Texte affiche avant `shown`, pour l'image a mi-bascule.
    previous: FlapText,
    /// Vrai si l'image affichee est l'image a mi-bascule (l'image finale suit).
    flipping: bool,
}

impl ClockApp {
    pub fn new() -> Self {
        Self {
            mode: Mode::Clock,
            stopwatch: Elapsed::default(),
            timer: Timer {
                duration_ms: 0,
                elapsed: Elapsed::default(),
                finished: false,
            },
            shown: FlapText::new(),
            previous: FlapText::new(),
            flipping: false,
        }
    }

    /// Texte a afficher en palettes, maintenant.
    fn flap_text(&self, now: u64) -> FlapText {
        let mut text = FlapText::new();
        match self.mode {
            Mode::Clock => match clock::now() {
                Some(dt) => {
                    let _ = core::fmt::write(&mut text, format_args!("{:02}:{:02}", dt.hour, dt.minute));
                }
                None => {
                    let _ = text.push_str("--:--");
                }
            },
            Mode::Stopwatch => {
                let elapsed = self.stopwatch.get(now);
                text = format_min_sec(elapsed);
                // Dixiemes : a l'arret (valeur exacte), ou en marche si la
                // frequence de mise a jour le permet.
                if !self.stopwatch.running() || STOPWATCH_REFRESH_MS < 1000 {
                    let tenths = if self.stopwatch.running() {
                        elapsed / STOPWATCH_REFRESH_MS * STOPWATCH_REFRESH_MS % 1000 / 100
                    } else {
                        elapsed % 1000 / 100
                    };
                    let _ = core::fmt::write(&mut text, format_args!(".{}", tenths));
                } else {
                    // En marche a 1 Hz : secondes entieres.
                    text = format_min_sec(elapsed / 1000 * 1000);
                }
            }
            Mode::Timer => {
                if self.timer.finished {
                    let _ = text.push_str("TERMINE");
                } else if self.timer.duration_ms == 0 {
                    let _ = text.push_str("--:--");
                } else {
                    // Arrondi a la seconde superieure : 4:59.2 restant s'affiche 05:00.
                    text = format_min_sec(self.timer.remaining(now).div_ceil(1000) * 1000);
                }
            }
        }
        text
    }

    /// Ligne d'etat sous les palettes.
    fn status_line(&self, canvas: &mut FrameBuffer) {
        match self.mode {
            Mode::Clock => match clock::now() {
                Some(dt) => {
                    let mut date = HeapString::<64>::new();
                    let _ = core::fmt::write(
                        &mut date,
                        format_args!(
                            "{} {} {} {} - semaine {}",
                            dt.weekday_name(),
                            dt.day,
                            dt.month_name(),
                            dt.year,
                            dt.iso_week()
                        ),
                    );
                    canvas.text_centered(date.as_str(), 380);
                }
                None => canvas.text_centered("Heure non reglee : TIME AAAA-MM-JJ HH:MM", 380),
            },
            Mode::Stopwatch => canvas.text_centered(
                if self.stopwatch.running() {
                    "En marche - STOP pour pause"
                } else if self.stopwatch.accumulated_ms == 0 {
                    "START pour demarrer"
                } else {
                    "En pause - START / RESET"
                },
                380,
            ),
            Mode::Timer => canvas.text_centered(
                if self.timer.finished {
                    "RESET pour effacer, MIN <n> pour relancer"
                } else if self.timer.duration_ms == 0 {
                    "MIN <n> pour lancer un minuteur de n minutes"
                } else if self.timer.elapsed.running() {
                    "En marche - STOP pour pause"
                } else {
                    "En pause - START / RESET"
                },
                380,
            ),
        }
    }

    fn handle_text(&mut self, text: &str, now: u64) -> AppCommand {
        match text {
            "HORLOGE" => self.mode = Mode::Clock,
            "CHRONO" => self.mode = Mode::Stopwatch,
            "MINUTEUR" => self.mode = Mode::Timer,
            "HELP" => {
                print_help();
                return AppCommand::Stay;
            }
            "START" | "STOP" | "RESET" => return self.control(text, now),
            _ => {
                if let Some(argument) = text.strip_prefix("MIN ") {
                    return self.set_timer(argument, now);
                }
                esp_println::println!("Commande inconnue. Tape HELP pour la liste des commandes.");
                return AppCommand::Stay;
            }
        }
        AppCommand::Redraw
    }

    /// START / STOP / RESET, appliques au chronometre ou au minuteur selon le mode.
    fn control(&mut self, command: &str, now: u64) -> AppCommand {
        match (self.mode, command) {
            (Mode::Clock, _) => {
                esp_println::println!("START/STOP/RESET : passe d'abord en mode CHRONO ou MINUTEUR.");
                return AppCommand::Stay;
            }
            (Mode::Stopwatch, "START") => self.stopwatch.start(now),
            (Mode::Stopwatch, "STOP") => {
                self.stopwatch.stop(now);
                return AppCommand::FullRedraw; // efface les traces du mode live
            }
            (Mode::Stopwatch, _) => self.stopwatch = Elapsed::default(),
            (Mode::Timer, "START") => {
                if self.timer.duration_ms == 0 || self.timer.finished {
                    esp_println::println!("Regle d'abord une duree : MIN <minutes>.");
                    return AppCommand::Stay;
                }
                self.timer.elapsed.start(now);
            }
            (Mode::Timer, "STOP") => {
                self.timer.elapsed.stop(now);
                return AppCommand::FullRedraw;
            }
            (Mode::Timer, _) => {
                self.timer.elapsed = Elapsed::default();
                self.timer.finished = false;
            }
        }
        AppCommand::Redraw
    }

    /// `MIN <n>` : regle le minuteur sur n minutes et le demarre.
    fn set_timer(&mut self, argument: &str, now: u64) -> AppCommand {
        match argument.trim().parse::<u64>() {
            Ok(minutes) if (1..=MAX_TIMER_MINUTES).contains(&minutes) => {
                self.mode = Mode::Timer;
                self.timer.duration_ms = minutes * 60_000;
                self.timer.elapsed = Elapsed::default();
                self.timer.elapsed.start(now);
                self.timer.finished = false;
                esp_println::println!("Minuteur lance : {} min.", minutes);
                AppCommand::Redraw
            }
            _ => {
                esp_println::println!("Usage : MIN <minutes> (1 a {}).", MAX_TIMER_MINUTES);
                AppCommand::Stay
            }
        }
    }

    fn render_tabs(&self, canvas: &mut FrameBuffer) {
        let mut tabs = HeapString::<48>::new();
        for mode in Mode::ALL {
            let _ = if mode == self.mode {
                core::fmt::write(&mut tabs, format_args!("[{}]  ", mode.label()))
            } else {
                core::fmt::write(&mut tabs, format_args!(" {}   ", mode.label()))
            };
        }
        canvas.text_centered(tabs.trim_end(), 60);
    }
}

/// "MM:SS" (minutes non bornees : "125:07").
fn format_min_sec(ms: u64) -> HeapString<16> {
    let secs = ms / 1000;
    let mut text = HeapString::new();
    let _ = core::fmt::write(&mut text, format_args!("{:02}:{:02}", secs / 60, secs % 60));
    text
}

fn print_help() {
    esp_println::println!("Commandes Horloge :");
    esp_println::println!("  LEFT / RIGHT       changer de mode (Horloge, Chrono, Minuteur)");
    esp_println::println!("  HORLOGE | CHRONO | MINUTEUR   aller directement a un mode");
    esp_println::println!("  START / STOP       demarrer / mettre en pause (chrono ou minuteur)");
    esp_println::println!("  RESET              remettre a zero (chrono ou minuteur)");
    esp_println::println!("  MIN <n>            lancer le minuteur pour n minutes");
    esp_println::println!("  TIME AAAA-MM-JJ HH:MM   regler l'heure (commande globale)");
    esp_println::println!("  HELP               afficher cette aide");
    esp_println::println!("  BACK               revenir au menu (chrono et minuteur continuent)");
}

impl App for ClockApp {
    fn name(&self) -> &'static str {
        "Horloge"
    }

    fn intro(&self) -> &'static str {
        "LEFT/RIGHT pour changer de mode, START/STOP/RESET, MIN <n>. HELP pour tout."
    }

    fn render(&self, canvas: &mut FrameBuffer) {
        self.render_tabs(canvas);

        let previous = if self.flipping { Some(self.previous.as_str()) } else { None };
        if self.shown.is_empty() {
            // Pas encore de tick depuis le demarrage : texte calcule a la volee.
            flap::draw_row(canvas, self.flap_text(uptime_ms()).as_str(), None, FLAP_CENTER_Y);
        } else {
            flap::draw_row(canvas, self.shown.as_str(), previous, FLAP_CENTER_Y);
        }

        self.status_line(canvas);
        canvas.text_centered("LEFT/RIGHT : mode | HELP : commandes | BACK : menu", 465);
    }

    fn handle_input(&mut self, input: Input) -> AppCommand {
        let now = uptime_ms();
        let command = match input {
            Input::Key(Key::Back) => return AppCommand::BackToMenu,
            Input::Key(Key::Left) => {
                self.mode = self.mode.previous();
                AppCommand::Redraw
            }
            Input::Key(Key::Right) => {
                self.mode = self.mode.next();
                AppCommand::Redraw
            }
            Input::Key(_) => AppCommand::Stay,
            Input::Text(text) => self.handle_text(text.trim(), now),
        };
        // L'ecran va etre redessine directement (sans animation) : memorise ce
        // qu'il affichera, pour que le prochain tick ne le redessine pas.
        self.shown = self.flap_text(now);
        self.flipping = false;
        command
    }

    fn tick(&mut self) -> AppCommand {
        let now = uptime_ms();

        // Fin du minuteur : verifiee dans tous les modes, meme hors de l'app.
        let mut just_finished = false;
        if self.timer.elapsed.running() && self.timer.remaining(now) == 0 {
            self.timer.elapsed.stop(now);
            self.timer.finished = true;
            just_finished = true;
            esp_println::println!("*** Minuteur termine ! ***");
        }

        let live = match self.mode {
            Mode::Stopwatch => self.stopwatch.running(),
            Mode::Timer => self.timer.elapsed.running(),
            Mode::Clock => true,
        };
        let command = if just_finished && self.mode == Mode::Timer {
            AppCommand::FullRedraw // "TERMINE" bien net
        } else if live {
            AppCommand::LiveRedraw
        } else {
            AppCommand::Redraw
        };

        // Image a mi-bascule affichee au tick precedent : afficher l'image finale.
        if self.flipping {
            self.flipping = false;
            return command;
        }

        let text = self.flap_text(now);
        if text == self.shown {
            return AppCommand::Stay;
        }
        self.previous = core::mem::replace(&mut self.shown, text);
        // Pas d'animation si la disposition change (ex. "00:03" -> "TERMINE").
        self.flipping = FLIP_ANIMATION
            && !just_finished
            && self.previous.chars().count() == self.shown.chars().count();
        command
    }
}
