//! Module LTE SIMCom A7682E : allumage et heure reseau.
//!
//! Le module recoit l'heure du reseau (NITZ) quand il s'attache, a condition
//! que la mise a jour automatique soit activee (`AT+CTZU=1`, envoye a chaque
//! demarrage par securite). On la lit ensuite avec `AT+CCLK?` : heure locale,
//! fuseau et heure d'ete inclus (valide avec Bouygues, cf. lte_at_console).
//!
//! Tout est NON BLOQUANT : `Lte::poll()` est appele a chaque tour de la
//! boucle principale et avance d'un pas (envoi d'une commande, lecture d'une
//! reponse...) sans jamais attendre. L'interface reste utilisable et le
//! watchdog nourri pendant les ~20-30 s d'attachement au reseau.
//!
//! Etapes :
//!   Probe -> (pas de reponse) -> impulsion PWRKEY -> demarrage du module
//!         -> Configure (ATE0, AT+CTZU=1) -> ReadClock (AT+CCLK?)
//!         -> Idle (resynchronisation toutes les heures)
//! Si l'heure n'arrive pas en 60 s (annee 70 = pas encore recue), on fait une
//! fois detacher/rattacher le module (AT+CFUN=0 / 1) pour qu'il la redemande.
//!
//! Notes materiel :
//! - Le module est sur VBATT : il reste allume quand l'ESP32 redemarre, d'ou le
//!   test "AT" avant toute impulsion PWRKEY.
//! - SIM1_DET n'est pas cablee et la detection a chaud est desactivee : apres
//!   avoir change de SIM, faire `AT+CRESET` (examples/lte_at_console.rs).

use esp_hal::Blocking;
use esp_hal::gpio::Output;
use esp_hal::time::{Duration, Instant};
use esp_hal::uart::Uart;
use heapless::Vec;

use crate::clock::DateTime;

/// Intervalle entre deux lectures de l'heure une fois reglee.
const RESYNC_INTERVAL: Duration = Duration::from_secs(3600);
/// Intervalle entre deux lectures tant que l'heure n'est pas arrivee.
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
/// Sans heure apres ce delai : un detacher/rattacher (une seule fois).
const REATTACH_AFTER: Duration = Duration::from_secs(60);
/// Duree de l'impulsion PWRKEY (A7682E : >= 50 ms pour allumer, < 2,5 s
/// pour ne pas eteindre un module deja allume).
const PWRKEY_PULSE: Duration = Duration::from_millis(800);
/// Temps laisse au module pour demarrer apres une impulsion.
const BOOT_TIMEOUT: Duration = Duration::from_secs(20);
/// Nombre d'impulsions PWRKEY avant d'abandonner.
const MAX_POWER_ATTEMPTS: u8 = 3;

const CONFIGURE: [&str; 2] = ["ATE0", "AT+CTZU=1"];
const REATTACH: [&str; 2] = ["AT+CFUN=0", "AT+CFUN=1"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Le module repond-il deja ?
    Probe,
    /// PWRKEY actif jusqu'a l'instant donne.
    PowerPulse { release_at: Instant },
    /// Le module demarre : "AT" toutes les secondes jusqu'a `give_up_at`.
    Booting { next_try: Instant, give_up_at: Instant },
    /// Envoi de CONFIGURE[index].
    Configure(usize),
    /// Envoi de REATTACH[index].
    Reattach(usize),
    /// Envoi de AT+CCLK?.
    ReadClock,
    /// Prochaine lecture de l'heure a l'instant donne.
    Idle { next_read: Instant },
    /// Module muet apres MAX_POWER_ATTEMPTS impulsions.
    Dead,
}

/// Commande envoyee dont on attend la reponse.
struct Pending {
    deadline: Instant,
}

pub struct Lte {
    uart: Uart<'static, Blocking>,
    pwrkey: Output<'static>,
    state: State,
    pending: Option<Pending>,
    response: Vec<u8, 256>,
    power_attempts: u8,
    /// Debut de l'attente de l'heure (pour REATTACH_AFTER).
    waiting_since: Option<Instant>,
    reattached: bool,
    synced_once: bool,
}

impl Lte {
    /// Prend l'UART et la broche PWRKEY donnees par `board_lte!(peripherals)`.
    pub fn new(uart: Uart<'static, Blocking>, pwrkey: Output<'static>) -> Self {
        Self {
            uart,
            pwrkey,
            state: State::Probe,
            pending: None,
            response: Vec::new(),
            power_attempts: 0,
            waiting_since: None,
            reattached: false,
            synced_once: false,
        }
    }

    /// Avance d'un pas. Renvoie l'heure du reseau quand elle vient d'etre lue
    /// (a la premiere synchronisation puis toutes les heures).
    pub fn poll(&mut self) -> Option<DateTime> {
        let now = Instant::now();
        self.drain_uart();

        // Une commande attend sa reponse : terminee, ou delai depasse ?
        if let Some(pending) = &self.pending {
            let complete = self.response.ends_with(b"\r\n")
                && (contains(&self.response, b"OK\r\n") || contains(&self.response, b"ERROR"));
            if !complete && now < pending.deadline {
                return None;
            }
            self.pending = None;
            let ok = contains(&self.response, b"OK\r\n");
            return self.on_response(ok, now);
        }

        match self.state {
            State::Probe => self.send("AT", Duration::from_millis(500)),
            State::PowerPulse { release_at } => {
                if now >= release_at {
                    self.pwrkey.set_low();
                    self.state = State::Booting {
                        next_try: now + Duration::from_secs(3),
                        give_up_at: now + BOOT_TIMEOUT,
                    };
                }
            }
            State::Booting { next_try, .. } => {
                if now >= next_try {
                    self.send("AT", Duration::from_millis(500));
                }
            }
            State::Configure(index) => self.send(CONFIGURE[index], Duration::from_secs(1)),
            State::Reattach(index) => self.send(REATTACH[index], Duration::from_secs(15)),
            State::ReadClock => self.send("AT+CCLK?", Duration::from_secs(1)),
            State::Idle { next_read } => {
                if now >= next_read {
                    self.state = State::ReadClock;
                }
            }
            State::Dead => {}
        }
        None
    }

    /// Traite la reponse (ou l'absence de reponse) a la commande envoyee.
    fn on_response(&mut self, ok: bool, now: Instant) -> Option<DateTime> {
        match self.state {
            State::Probe => {
                if ok {
                    esp_println::println!("LTE : module deja allume.");
                    self.state = State::Configure(0);
                } else {
                    self.start_power_pulse(now);
                }
            }
            State::Booting { give_up_at, .. } => {
                if ok {
                    esp_println::println!("LTE : module demarre.");
                    self.state = State::Configure(0);
                } else if now >= give_up_at {
                    self.start_power_pulse(now);
                } else {
                    self.state = State::Booting {
                        next_try: now + Duration::from_secs(1),
                        give_up_at,
                    };
                }
            }
            State::Configure(index) => {
                if !ok {
                    esp_println::println!("LTE : {} sans OK, on continue.", CONFIGURE[index]);
                }
                self.state = if index + 1 < CONFIGURE.len() {
                    State::Configure(index + 1)
                } else {
                    esp_println::println!("LTE : en attente de l'heure du reseau...");
                    self.waiting_since = Some(now);
                    State::ReadClock
                };
            }
            State::Reattach(index) => {
                self.state = if index + 1 < REATTACH.len() {
                    State::Reattach(index + 1)
                } else {
                    State::Idle { next_read: now + RETRY_INTERVAL }
                };
            }
            State::ReadClock => {
                if let Some(time) = parse_cclk(&self.response) {
                    if !self.synced_once {
                        esp_println::println!("LTE : heure reglee par le reseau : {}", time);
                        self.synced_once = true;
                    }
                    self.waiting_since = None;
                    self.state = State::Idle { next_read: now + RESYNC_INTERVAL };
                    return Some(time);
                }
                // Pas encore d'heure (annee 70) ou reponse illisible.
                let since = *self.waiting_since.get_or_insert(now);
                if !self.reattached && now >= since + REATTACH_AFTER {
                    esp_println::println!("LTE : pas d'heure apres 60 s, detacher/rattacher (AT+CFUN=0/1).");
                    self.reattached = true;
                    self.state = State::Reattach(0);
                } else {
                    self.state = State::Idle { next_read: now + RETRY_INTERVAL };
                }
            }
            // Pas de commande envoyee dans ces etats.
            State::PowerPulse { .. } | State::Idle { .. } | State::Dead => {}
        }
        None
    }

    fn start_power_pulse(&mut self, now: Instant) {
        if self.power_attempts >= MAX_POWER_ATTEMPTS {
            esp_println::println!(
                "LTE : aucune reponse apres {} impulsions PWRKEY, abandon (batterie ? cablage ?).",
                MAX_POWER_ATTEMPTS
            );
            self.state = State::Dead;
            return;
        }
        self.power_attempts += 1;
        esp_println::println!(
            "LTE : pas de reponse, impulsion PWRKEY ({}/{}).",
            self.power_attempts,
            MAX_POWER_ATTEMPTS
        );
        self.pwrkey.set_high(); // Q5 (2N7002) tire PWRKEY du module a la masse
        self.state = State::PowerPulse { release_at: now + PWRKEY_PULSE };
    }

    /// Envoie une commande AT ; la reponse est attendue par `poll()`.
    fn send(&mut self, command: &str, timeout: Duration) {
        self.response.clear();
        let _ = self.uart.write(command.as_bytes());
        let _ = self.uart.write(b"\r\n");
        self.pending = Some(Pending { deadline: Instant::now() + timeout });
    }

    /// Lit les octets recus. Hors commande en cours, ce sont des messages
    /// spontanes du module (+CTZV, SMS DONE...) : ignores pour l'instant.
    fn drain_uart(&mut self) {
        let mut buffer = [0u8; 64];
        while self.uart.read_ready() {
            let Ok(n) = self.uart.read(&mut buffer) else { break };
            if n == 0 {
                break;
            }
            if self.pending.is_some() {
                // Si le tampon est plein, la fin est perdue (reponses courtes ici).
                let room = self.response.capacity() - self.response.len();
                let _ = self.response.extend_from_slice(&buffer[..n.min(room)]);
            }
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Extrait l'heure de `+CCLK: "yy/MM/dd,hh:mm:ss+zz"` (heure locale ; le
/// fuseau zz, en quarts d'heure, est ignore). `None` si l'annee n'est pas
/// entre 2025 et 2069 : le module renvoie "70/01/01" tant qu'il n'a pas recu
/// l'heure.
fn parse_cclk(response: &[u8]) -> Option<DateTime> {
    const PREFIX: &[u8] = b"+CCLK: \"";
    let start = response.windows(PREFIX.len()).position(|w| w == PREFIX)? + PREFIX.len();
    let text = response.get(start..start + 17)?; // "yy/MM/dd,hh:mm:ss"
    let field = |at: usize| -> Option<u8> {
        let (tens, units) = (text[at], text[at + 1]);
        if tens.is_ascii_digit() && units.is_ascii_digit() {
            Some((tens - b'0') * 10 + (units - b'0'))
        } else {
            None
        }
    };
    // "70" = 1970, l'origine de l'horloge du module : pas d'heure recue.
    let year = field(0)?;
    if !(25..70).contains(&year) {
        return None;
    }
    DateTime::new(2000 + year as u16, field(3)?, field(6)?, field(9)?, field(12)?, field(15)?)
}
