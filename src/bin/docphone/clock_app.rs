//! App Horloge : heure en grand, chronometre et minuteur.
//!
//! Chaque mode ne redessine que lorsque l'affichage change :
//! - Horloge : chaque minute (rafraichissement partiel normal) ;
//! - Chronometre et minuteur en marche : chaque seconde, en mode "live"
//!   (`AppCommand::LiveRedraw` : partiel, avec beaucoup moins de
//!   rafraichissements complets) ; STOP fait un rafraichissement complet pour
//!   effacer les traces et afficher la valeur exacte (1/10 s pour le chrono).
//!
//! Chronometre et minuteur utilisent le compteur depuis le demarrage : ils
//! fonctionnent meme si l'heure n'est pas reglee, et continuent quand on
//! revient au menu.

use esp32_quick::FrameBuffer;
use esp32_quick::clock::{self, uptime_ms};
use esp32_quick::input::{Input, Key};
use heapless::String as HeapString;

use super::app::{App, AppCommand};

/// Taille des gros chiffres (police 10x20 agrandie : 60 x 120 px par caractere).
const BIG_FACTOR: u32 = 6;
const BIG_Y: i32 = 300;

/// Intervalle de mise a jour du chronometre en marche, en ms. En dessous de
/// 1000, les dixiemes de seconde sont aussi affiches (essayer 333 pour ~3 Hz).
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

pub struct ClockApp {
    mode: Mode,
    stopwatch: Elapsed,
    timer: Timer,
    /// Ce qui est actuellement affiche (voir `display_key`) : on ne redessine
    /// que quand cette valeur change.
    shown: (Mode, u64),
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
            shown: (Mode::Clock, u64::MAX),
        }
    }

    /// Valeur qui change exactement quand l'ecran doit etre redessine.
    fn display_key(&self, now: u64) -> (Mode, u64) {
        let value = match self.mode {
            // Minute courante (u64::MAX - 1 si l'heure n'est pas reglee).
            Mode::Clock => clock::now().map_or(u64::MAX - 1, |dt| dt.to_secs() / 60),
            Mode::Stopwatch if self.stopwatch.running() => self.stopwatch.get(now) / STOPWATCH_REFRESH_MS,
            Mode::Stopwatch => self.stopwatch.get(now),
            Mode::Timer if self.timer.finished => u64::MAX - 2,
            Mode::Timer => {
                let remaining = self.timer.remaining(now);
                if self.timer.elapsed.running() { remaining.div_ceil(1000) } else { remaining }
            }
        };
        (self.mode, value)
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
        let now = uptime_ms();
        self.render_tabs(canvas);

        match self.mode {
            Mode::Clock => match clock::now() {
                Some(dt) => {
                    let mut time = HeapString::<8>::new();
                    let _ = core::fmt::write(&mut time, format_args!("{:02}:{:02}", dt.hour, dt.minute));
                    canvas.text_big_centered(time.as_str(), BIG_Y, BIG_FACTOR);

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
                None => {
                    canvas.text_big_centered("--:--", BIG_Y, BIG_FACTOR);
                    canvas.text_centered("Heure non reglee : TIME AAAA-MM-JJ HH:MM", 380);
                }
            },
            Mode::Stopwatch => {
                let elapsed = self.stopwatch.get(now);
                let mut text = format_min_sec(elapsed);
                let status = if self.stopwatch.running() {
                    if STOPWATCH_REFRESH_MS < 1000 {
                        let _ = core::fmt::write(&mut text, format_args!(".{}", elapsed % 1000 / 100));
                    }
                    "En marche - STOP pour pause"
                } else {
                    // A l'arret : valeur exacte, au dixieme de seconde.
                    let _ = core::fmt::write(&mut text, format_args!(".{}", elapsed % 1000 / 100));
                    if elapsed == 0 { "START pour demarrer" } else { "En pause - START / RESET" }
                };
                canvas.text_big_centered(text.as_str(), BIG_Y, BIG_FACTOR);
                canvas.text_centered(status, 380);
            }
            Mode::Timer => {
                if self.timer.finished {
                    canvas.text_big_centered("Termine", BIG_Y, BIG_FACTOR);
                    canvas.text_centered("RESET pour effacer, MIN <n> pour relancer", 380);
                } else if self.timer.duration_ms == 0 {
                    canvas.text_big_centered("--:--", BIG_Y, BIG_FACTOR);
                    canvas.text_centered("MIN <n> pour lancer un minuteur de n minutes", 380);
                } else {
                    // Arrondi a la seconde superieure : 4:59.2 restant s'affiche 05:00.
                    let remaining = self.timer.remaining(now).div_ceil(1000) * 1000;
                    canvas.text_big_centered(format_min_sec(remaining).as_str(), BIG_Y, BIG_FACTOR);
                    canvas.text_centered(
                        if self.timer.elapsed.running() { "En marche - STOP pour pause" } else { "En pause - START / RESET" },
                        380,
                    );
                }
            }
        }

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
        // L'ecran va etre redessine : memorise ce qu'il affichera, pour que le
        // prochain tick ne le redessine pas une seconde fois.
        self.shown = self.display_key(now);
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

        let key = self.display_key(now);
        if key == self.shown {
            return AppCommand::Stay;
        }
        self.shown = key;

        let live = match self.mode {
            Mode::Stopwatch => self.stopwatch.running(),
            Mode::Timer => self.timer.elapsed.running(),
            Mode::Clock => false,
        };
        if live {
            AppCommand::LiveRedraw
        } else if just_finished && self.mode == Mode::Timer {
            AppCommand::FullRedraw // "Termine" bien net
        } else {
            AppCommand::Redraw
        }
    }
}
