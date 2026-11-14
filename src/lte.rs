//! Module LTE SIMCom A7682E : allumage, heure reseau, lecture des SMS.
//!
//! Heure : le module recoit l'heure du reseau (NITZ) quand il s'attache, si la
//! mise a jour automatique est activee (`AT+CTZU=1`, envoye a chaque demarrage
//! par securite). On la lit avec `AT+CCLK?` : heure locale, fuseau et heure
//! d'ete inclus (valide avec Bouygues, cf. examples/lte_at_console.rs).
//!
//! SMS : mode texte (`AT+CMGF=1`), jeu de caracteres UCS2 (`AT+CSCS="UCS2"` :
//! numeros et textes arrivent en hexadecimal UTF-16, decodes ici, ce qui garde
//! accents et emojis). Le module signale chaque nouveau SMS par
//! `+CMTI: "SM",<n>` (`AT+CNMI=2,1`) : on le lit alors et on l'affiche sur la
//! console. Les SMS restent stockes dans le module (lister, lire, supprimer
//! avec `request()`). Envoi avec `send_sms()` : toujours en UCS2
//! (`AT+CSMP=17,167,0,8`), pour que accents et emojis passent sans dependre de
//! la conversion GSM du module ; limite a 70 caracteres (un seul SMS).
//!
//! Tout est NON BLOQUANT : `Lte::poll()` est appele a chaque tour de la
//! boucle principale et avance d'un pas (envoi d'une commande, lecture des
//! lignes recues...) sans jamais attendre. L'interface reste utilisable et le
//! watchdog nourri pendant les ~20-30 s d'attachement au reseau.
//!
//! Etapes :
//!   Probe -> (pas de reponse) -> impulsion PWRKEY -> demarrage du module
//!         -> Configure -> ReadClock (AT+CCLK?) -> Idle
//! En Idle : demandes SMS en attente, puis relecture de l'heure (toutes les
//! 5 s tant qu'elle n'est pas arrivee, puis toutes les heures). Sans heure
//! apres 60 s (annee 70 = pas encore recue), on fait une fois
//! detacher/rattacher le module (AT+CFUN=0 / 1) pour qu'il la redemande.
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
use heapless::{Deque, String, Vec};

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
/// Delai de reponse pour les commandes SMS (une longue liste peut prendre
/// quelques secondes).
const SMS_TIMEOUT: Duration = Duration::from_secs(10);
/// Delai d'envoi d'un SMS (AT+CMGS : jusqu'a 60 s selon le reseau).
const SEND_TIMEOUT: Duration = Duration::from_secs(60);
/// Longueur maximale d'un SMS envoye, en unites UTF-16 (un SMS UCS2 = 140
/// octets ; un emoji compte pour 2).
pub const SMS_MAX_UNITS: usize = 70;
/// Longueur maximale d'un numero (format international : + et 15 chiffres).
pub const NUMBER_MAX_LEN: usize = 16;
/// Longueur affichee du texte dans la liste des SMS.
const LIST_PREVIEW_CHARS: usize = 50;

const CONFIGURE: [&str; 6] = [
    "ATE0",                // pas d'echo des commandes
    "AT+CTZU=1",           // heure reseau automatique (NITZ)
    "AT+CMGF=1",           // SMS en mode texte
    "AT+CSCS=\"UCS2\"",    // textes et numeros en UTF-16 hexadecimal
    "AT+CNMI=2,1,0,0,0",   // signaler les nouveaux SMS par +CMTI
    "AT+CSMP=17,167,0,8",  // SMS envoyes en UCS2 (dcs=8)
];
const REATTACH: [&str; 2] = ["AT+CFUN=0", "AT+CFUN=1"];
/// Commandes de `Request::Status`, envoyees l'une apres l'autre (une erreur
/// n'arrete pas la suite).
const STATUS: [&str; 6] = [
    "AT+CPIN?",     // etat de la SIM
    "AT+CEREG?",    // enregistrement sur le reseau 4G
    "AT+COPS?",     // operateur et technologie radio
    "AT+CSQ",       // niveau de signal
    "AT+CBC",       // tension de la batterie (VBATT) vue par le module
    "AT+CPMUTEMP",  // temperature du module
];

/// Demande a traiter quand le module est pret (commandes SMS de la console).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Lister tous les SMS stockes (apercu du texte).
    ListSms,
    /// Afficher le SMS numero n en entier.
    ReadSms(u16),
    /// Supprimer le SMS numero n.
    DeleteSms(u16),
    /// Envoyer le SMS prepare par `send_sms()`.
    SendSms,
    /// Etat du module : SIM, reseau, operateur, signal, batterie,
    /// temperature (STATUS[etape]).
    Status(u8),
}

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
    /// Traitement d'une demande de la console.
    Serving(Request),
    /// Pret : demandes en attente, puis heure a `next_clock_read`.
    Idle,
    /// Module muet apres MAX_POWER_ATTEMPTS impulsions.
    Dead,
}

/// Ce qu'on attend apres un en-tete +CMGL / +CMGR : la ligne du texte.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Body {
    None,
    /// Texte d'un SMS de la liste : apercu sur une ligne.
    Preview,
    /// Texte d'un SMS lu en entier.
    Full,
}

pub struct Lte {
    uart: Uart<'static, Blocking>,
    pwrkey: Output<'static>,
    state: State,
    /// Une commande a ete envoyee ; reponse attendue avant cet instant.
    deadline: Option<Instant>,
    /// Reponse de la commande en cours : OK, ERROR.
    got_ok: bool,
    got_error: bool,
    /// Heure lue dans la reponse a AT+CCLK?.
    clock_reading: Option<DateTime>,
    /// Ligne en cours de reception. 700 octets : un SMS de 160 caracteres
    /// fait 640 caracteres en UCS2 hexadecimal.
    line: Vec<u8, 704>,
    /// Le texte d'un SMS est sur la ligne qui suit son en-tete.
    body: Body,
    /// SMS comptes dans la reponse a AT+CMGL.
    listed: u16,
    /// AT+CMGL avec "ALL" en clair plutot qu'en UCS2 (selon le firmware du
    /// module ; bascule automatique si la forme UCS2 est refusee).
    list_plain: bool,
    requests: Deque<Request, 8>,
    /// SMS a envoyer (numero, texte), un seul a la fois.
    outgoing: Option<(String<NUMBER_MAX_LEN>, String<{ SMS_MAX_UNITS * 4 }>)>,
    /// AT+CMGS envoye : le module va repondre "> " (sans fin de ligne), et
    /// on enverra alors le texte.
    awaiting_prompt: bool,
    /// Derniere ligne d'erreur du module (ex. "+CMS ERROR: 500").
    last_error: String<48>,
    next_clock_read: Instant,
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
            deadline: None,
            got_ok: false,
            got_error: false,
            clock_reading: None,
            line: Vec::new(),
            body: Body::None,
            listed: 0,
            list_plain: false,
            requests: Deque::new(),
            outgoing: None,
            awaiting_prompt: false,
            last_error: String::new(),
            next_clock_read: Instant::now(),
            power_attempts: 0,
            waiting_since: None,
            reattached: false,
            synced_once: false,
        }
    }

    /// Met une demande en file ; elle sera traitee des que le module est
    /// pret. Faux (et message) si le module est hors service ou la file pleine.
    pub fn request(&mut self, request: Request) -> bool {
        if self.state == State::Dead {
            esp_println::println!("LTE : module hors service (pas de reponse au demarrage).");
            return false;
        }
        if self.requests.push_back(request).is_err() {
            esp_println::println!("LTE : trop de demandes en attente, reessaie.");
            return false;
        }
        if !matches!(self.state, State::Idle | State::Serving(_) | State::ReadClock) {
            esp_println::println!("LTE : module en cours de demarrage, demande traitee des qu'il est pret.");
        }
        true
    }

    /// Prepare l'envoi d'un SMS. Faux (et message) si le numero ou le texte
    /// est invalide, ou si un envoi est deja en cours.
    pub fn send_sms(&mut self, number: &str, text: &str) -> bool {
        let digits = number.strip_prefix('+').unwrap_or(number);
        if digits.len() < 3 || number.len() > NUMBER_MAX_LEN || !digits.bytes().all(|b| b.is_ascii_digit()) {
            esp_println::println!("Numero invalide : {} (ex. +33612345678 ou 0612345678).", number);
            return false;
        }
        let units = text.encode_utf16().count();
        if units == 0 || units > SMS_MAX_UNITS {
            esp_println::println!(
                "Texte de {} caracteres : entre 1 et {} (un emoji compte pour 2).",
                units,
                SMS_MAX_UNITS
            );
            return false;
        }
        if self.outgoing.is_some() {
            esp_println::println!("Un SMS est deja en cours d'envoi, reessaie dans un instant.");
            return false;
        }
        let (Ok(number), Ok(text)) = (String::try_from(number), String::try_from(text)) else {
            esp_println::println!("Texte trop long.");
            return false;
        };
        self.outgoing = Some((number, text));
        if !self.request(Request::SendSms) {
            self.outgoing = None;
            return false;
        }
        true
    }

    /// Avance d'un pas. Renvoie l'heure du reseau quand elle vient d'etre lue
    /// (a la premiere synchronisation puis toutes les heures).
    pub fn poll(&mut self) -> Option<DateTime> {
        let now = Instant::now();
        self.read_lines();

        // Une commande attend sa reponse : terminee, ou delai depasse ?
        if let Some(deadline) = self.deadline {
            if !self.got_ok && !self.got_error && now < deadline {
                return None;
            }
            self.deadline = None;
            if self.awaiting_prompt {
                // Invite "> " jamais recue : ESC annule la saisie cote module.
                self.awaiting_prompt = false;
                let _ = self.uart.write(&[0x1B]);
            }
            return self.on_response(now);
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
            State::ReadClock => {
                self.clock_reading = None;
                self.send("AT+CCLK?", Duration::from_secs(1));
            }
            State::Serving(request) => self.send_request(request),
            State::Idle => {
                if let Some(request) = self.requests.pop_front() {
                    self.state = State::Serving(request);
                } else if now >= self.next_clock_read {
                    self.state = State::ReadClock;
                }
            }
            State::Dead => {}
        }
        None
    }

    /// Traite la fin de la commande envoyee (OK, ERROR ou delai depasse).
    fn on_response(&mut self, now: Instant) -> Option<DateTime> {
        let ok = self.got_ok;
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
                if index + 1 < REATTACH.len() {
                    self.state = State::Reattach(index + 1);
                } else {
                    self.next_clock_read = now + RETRY_INTERVAL;
                    self.state = State::Idle;
                }
            }
            State::ReadClock => {
                self.state = State::Idle;
                if let Some(time) = self.clock_reading.take() {
                    if !self.synced_once {
                        esp_println::println!("LTE : heure reglee par le reseau : {}", time);
                        self.synced_once = true;
                    }
                    self.waiting_since = None;
                    self.next_clock_read = now + RESYNC_INTERVAL;
                    return Some(time);
                }
                // Pas encore d'heure (annee 70) ou reponse illisible.
                let since = *self.waiting_since.get_or_insert(now);
                if !self.reattached && now >= since + REATTACH_AFTER {
                    esp_println::println!("LTE : pas d'heure apres 60 s, detacher/rattacher (AT+CFUN=0/1).");
                    self.reattached = true;
                    self.state = State::Reattach(0);
                } else {
                    self.next_clock_read = now + RETRY_INTERVAL;
                }
            }
            State::Serving(request) => {
                self.finish_request(request);
                self.state = State::Idle;
            }
            // Pas de commande envoyee dans ces etats.
            State::PowerPulse { .. } | State::Idle | State::Dead => {}
        }
        None
    }

    fn send_request(&mut self, request: Request) {
        let mut command = String::<96>::new();
        let _ = match request {
            Request::ListSms => {
                self.listed = 0;
                if !self.list_plain {
                    esp_println::println!("--- SMS stockes ---");
                    // "ALL" en UCS2, puisque les parametres texte suivent AT+CSCS.
                    command.push_str("AT+CMGL=\"0041004C004C\"")
                } else {
                    command.push_str("AT+CMGL=\"ALL\"")
                }
            }
            Request::ReadSms(index) => core::fmt::write(&mut command, format_args!("AT+CMGR={}", index))
                .map_err(|_| ()),
            Request::DeleteSms(index) => core::fmt::write(&mut command, format_args!("AT+CMGD={}", index))
                .map_err(|_| ()),
            Request::Status(step) => {
                if step == 0 {
                    esp_println::println!("--- Etat du module LTE ---");
                }
                command.push_str(STATUS[step as usize])
            }
            Request::SendSms => {
                let Some((number, _)) = &self.outgoing else {
                    self.state = State::Idle;
                    return;
                };
                esp_println::println!("Envoi du SMS a {}...", number);
                // Numero en UCS2, comme tous les parametres texte (AT+CSCS).
                let _ = command.push_str("AT+CMGS=\"");
                let _ = write_ucs2_hex(&mut command, number);
                command.push_str("\"")
            }
        };
        if request == Request::SendSms {
            self.send(&command, SEND_TIMEOUT);
            self.awaiting_prompt = true;
        } else {
            self.send(&command, SMS_TIMEOUT);
        }
    }

    fn finish_request(&mut self, request: Request) {
        let failed = !self.got_ok;
        if request == Request::ListSms && failed && self.got_error && !self.list_plain {
            // Forme UCS2 refusee : on retente tout de suite avec "ALL" en clair.
            self.list_plain = true;
            let _ = self.requests.push_front(Request::ListSms);
            return;
        }
        if let Request::Status(step) = request {
            if failed {
                esp_println::println!("{} : {}", STATUS[step as usize], self.error_label());
            }
            if (step as usize) + 1 < STATUS.len() {
                let _ = self.requests.push_front(Request::Status(step + 1));
            } else {
                esp_println::println!("---");
            }
            return;
        }
        if request == Request::SendSms {
            if let Some((number, _)) = self.outgoing.take() {
                if failed {
                    esp_println::println!("Echec de l'envoi a {} : {}", number, self.error_label());
                } else {
                    esp_println::println!("SMS envoye a {}.", number);
                }
            }
            return;
        }
        match request {
            Request::ListSms if !failed => {
                if self.listed == 0 {
                    esp_println::println!("Aucun SMS.");
                }
                esp_println::println!("--- {} SMS. Lire : SMS LIRE <n>, supprimer : SMS SUPPR <n> ---", self.listed);
            }
            Request::DeleteSms(index) if !failed => esp_println::println!("SMS {} supprime.", index),
            Request::ReadSms(index) if failed => esp_println::println!("SMS {} : introuvable.", index),
            _ if failed && !self.got_error => esp_println::println!("LTE : pas de reponse du module."),
            _ if failed => esp_println::println!("LTE : le module a refuse la commande ({}).", self.error_label()),
            _ => {}
        }
    }

    /// Derniere erreur du module, ou "pas de reponse" (delai depasse).
    fn error_label(&self) -> &str {
        if !self.got_error {
            "pas de reponse du module"
        } else if self.last_error.is_empty() {
            "ERROR"
        } else {
            &self.last_error
        }
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
        self.got_ok = false;
        self.got_error = false;
        self.last_error.clear();
        self.body = Body::None;
        let _ = self.uart.write(command.as_bytes());
        let _ = self.uart.write(b"\r\n");
        self.deadline = Some(Instant::now() + timeout);
    }

    /// Ecrit le texte du SMS en UCS2 hexadecimal, puis Ctrl+Z (envoi).
    fn write_outgoing_text(&mut self) {
        let Some((_, text)) = &self.outgoing else {
            let _ = self.uart.write(&[0x1B]); // rien a envoyer : annule
            return;
        };
        let mut hex = String::<{ SMS_MAX_UNITS * 4 }>::new();
        let _ = write_ucs2_hex(&mut hex, text);
        let _ = self.uart.write(hex.as_bytes());
        let _ = self.uart.write(&[0x1A]);
    }

    /// Lit les octets recus et traite chaque ligne complete.
    fn read_lines(&mut self) {
        let mut buffer = [0u8; 64];
        while self.uart.read_ready() {
            let Ok(n) = self.uart.read(&mut buffer) else { break };
            if n == 0 {
                break;
            }
            for &byte in &buffer[..n] {
                match byte {
                    b'\n' => {
                        let line = core::mem::take(&mut self.line);
                        if let Ok(text) = core::str::from_utf8(&line) {
                            self.handle_line(text.trim());
                        }
                    }
                    b'\r' => {}
                    // Ligne trop longue : la fin est perdue.
                    _ => {
                        let _ = self.line.push(byte);
                        // Invite de saisie du texte apres AT+CMGS ("> ", sans
                        // fin de ligne) : texte en UCS2 puis Ctrl+Z pour envoyer.
                        if self.awaiting_prompt && self.line.as_slice() == b"> " {
                            self.awaiting_prompt = false;
                            self.line.clear();
                            self.write_outgoing_text();
                        }
                    }
                }
            }
        }
    }

    /// Traite une ligne : reponse a la commande en cours ou message spontane.
    fn handle_line(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }

        // Texte d'un SMS : la ligne qui suit son en-tete (sauf SMS vide, ou
        // la ligne suivante est deja le OK final).
        if self.body != Body::None && line != "OK" {
            let body = self.body;
            self.body = Body::None;
            match body {
                Body::Preview => esp_println::println!("     {}", Decoded::preview(line, LIST_PREVIEW_CHARS)),
                _ => esp_println::println!("{}", Decoded::full(line)),
            }
            return;
        }

        if line == "OK" {
            self.got_ok = true;
        } else if line == "ERROR" || line.starts_with("+CME ERROR") || line.starts_with("+CMS ERROR") {
            self.got_error = true;
            self.last_error.clear();
            let _ = self.last_error.push_str(&line[..line.len().min(48)]);
        } else if let Some(rest) = line.strip_prefix("+CCLK: ") {
            self.clock_reading = parse_cclk(rest);
        } else if let Some(rest) = line.strip_prefix("+CMTI: ") {
            // Nouveau SMS : +CMTI: "SM",<n> -> on le lit tout de suite.
            if let Some(index) = rest.rsplit(',').next().and_then(|n| n.trim().parse().ok()) {
                esp_println::println!();
                esp_println::println!("*** Nouveau SMS (numero {}) ***", index);
                let _ = self.requests.push_back(Request::ReadSms(index));
            }
        } else if let Some(rest) = line.strip_prefix("+CMGL: ") {
            // +CMGL: <n>,"<etat>","<numero>","<nom>","<date>"
            let fields = split_fields(rest);
            if fields.len() >= 5 {
                self.listed += 1;
                esp_println::println!(
                    "[{}] {} {} {}",
                    fields[0],
                    status_label(fields[1]),
                    Decoded::full(fields[2]),
                    DateLabel(fields[4])
                );
                self.body = Body::Preview;
            }
        } else if let Some(rest) = line.strip_prefix("+CMGR: ") {
            // +CMGR: "<etat>","<numero>","<nom>","<date>"
            let fields = split_fields(rest);
            if fields.len() >= 4 {
                esp_println::println!("De : {}   le {}", Decoded::full(fields[1]), DateLabel(fields[3]));
                self.body = Body::Full;
            }
        }
        // Reponses a `Request::Status` (affichees seulement pendant l'etat :
        // +CPIN arrive aussi spontanement au demarrage du module).
        else if matches!(self.state, State::Serving(Request::Status(_))) {
            print_status_line(line);
        }
        // Autres messages spontanes (+CTZV, SMS DONE, +CGEV...) : ignores.
    }
}

/// Affiche en clair une reponse de `Request::Status`.
fn print_status_line(line: &str) {
    if let Some(rest) = line.strip_prefix("+CPIN: ") {
        let label = if rest == "READY" { "prete" } else { rest };
        esp_println::println!("SIM          : {}", label);
    } else if let Some(rest) = line.strip_prefix("+CEREG: ") {
        // +CEREG: <n>,<stat>[,...]
        let stat = split_fields(rest).get(1).copied().unwrap_or("");
        let label = match stat {
            "0" => "non enregistre",
            "1" => "enregistre",
            "2" => "recherche en cours",
            "3" => "refuse par le reseau",
            "5" => "enregistre (itinerance)",
            _ => "inconnu",
        };
        esp_println::println!("Reseau 4G    : {}", label);
    } else if let Some(rest) = line.strip_prefix("+COPS: ") {
        // +COPS: <mode>,<format>,"<operateur>",<techno>
        let fields = split_fields(rest);
        let mut code = String::<16>::new();
        let _ = core::fmt::write(&mut code, format_args!("{}", Decoded::full(fields.get(2).copied().unwrap_or(""))));
        let name = match code.as_str() {
            "" => "aucun",
            "20801" | "20802" => "Orange",
            "20810" | "20811" | "20813" => "SFR",
            "20815" | "20816" => "Free",
            "20820" | "20821" | "20888" => "Bouygues Telecom",
            other => other,
        };
        let technology = match fields.get(3).copied().unwrap_or("") {
            "0" | "1" | "3" => "2G",
            "2" | "4" | "5" | "6" => "3G",
            "7" => "4G",
            "" => "",
            _ => "autre",
        };
        esp_println::println!("Operateur    : {} {} ({})", name, technology, code);
    } else if let Some(rest) = line.strip_prefix("+CSQ: ") {
        // +CSQ: <rssi>,<ber> ; rssi 0..31 -> -113..-51 dBm, 99 = inconnu
        let rssi: u8 = split_fields(rest).first().and_then(|r| r.parse().ok()).unwrap_or(99);
        if rssi > 31 {
            esp_println::println!("Signal       : inconnu");
        } else {
            let dbm = -113 + 2 * rssi as i32;
            let bars = match rssi {
                0..=5 => 0,
                6..=9 => 1,
                10..=14 => 2,
                15..=19 => 3,
                20..=24 => 4,
                _ => 5,
            };
            let mut gauge = String::<8>::new();
            for i in 0..5 {
                let _ = gauge.push(if i < bars { '#' } else { '.' });
            }
            esp_println::println!("Signal       : [{}] {} dBm (CSQ {})", gauge, dbm, rssi);
        }
    } else if let Some(rest) = line.strip_prefix("+CBC: ") {
        esp_println::println!("Batterie     : {} (vue par le module, sur VBATT)", rest);
    } else if let Some(rest) = line.strip_prefix("+CPMUTEMP: ") {
        esp_println::println!("Temperature  : {} degC (module)", rest.trim());
    }
}

/// Ecrit `text` en UCS2 hexadecimal (UTF-16, 4 chiffres par unite).
fn write_ucs2_hex(out: &mut impl core::fmt::Write, text: &str) -> core::fmt::Result {
    for unit in text.encode_utf16() {
        write!(out, "{:04X}", unit)?;
    }
    Ok(())
}

/// Decoupe "a","b",c en champs (virgules hors guillemets), sans guillemets.
fn split_fields(text: &str) -> Vec<&str, 8> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                let _ = fields.push(text[start..i].trim_matches('"'));
                start = i + 1;
            }
            _ => {}
        }
    }
    let _ = fields.push(text[start..].trim_matches('"'));
    fields
}

fn status_label(status: &str) -> &'static str {
    // L'etat peut arriver en UCS2 selon le firmware : decode d'abord.
    let mut decoded = String::<16>::new();
    let _ = core::fmt::write(&mut decoded, format_args!("{}", Decoded::full(status)));
    match decoded.as_str() {
        "REC UNREAD" => "NON LU",
        "REC READ" => "lu    ",
        "STO SENT" => "envoye",
        "STO UNSENT" => "brouil",
        _ => "?     ",
    }
}

/// Affiche "26/11/14,14:02:11+04" sous la forme "14/11/2026 14:02".
struct DateLabel<'a>(&'a str);

impl core::fmt::Display for DateLabel<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let d = self.0;
        match (d.get(0..2), d.get(3..5), d.get(6..8), d.get(9..14)) {
            (Some(y), Some(m), Some(day), Some(hm)) => write!(f, "{}/{}/20{} {}", day, m, y, hm),
            _ => f.write_str(d),
        }
    }
}

/// Texte UCS2 (UTF-16 en hexadecimal, ex. "00480069" = "Hi") affiche en
/// clair. Si le texte n'est pas de l'hexadecimal UCS2, il est affiche tel quel.
///
/// PARTICULARITE DU A7682E : pour un SMS recu dans l'alphabet GSM 7 bits (le
/// cas de la plupart des SMS sans emoji), le module ne convertit pas en vrai
/// UCS2 mais recopie les codes GSM bruts : "é" arrive en 0005 au lieu de
/// 00E9 (constate : "r0005server" pour "réserver"). Si toutes les valeurs sont
/// < 0x80, le texte est donc decode avec la table GSM 03.38 ; sinon c'est un
/// vrai SMS UCS2 (emojis, caracteres hors alphabet GSM), decode en UTF-16.
struct Decoded<'a> {
    hex: &'a str,
    /// Nombre maximal de caracteres affiches (`None` = tout).
    limit: Option<usize>,
}

impl<'a> Decoded<'a> {
    fn full(hex: &'a str) -> Self {
        Self { hex, limit: None }
    }

    fn preview(hex: &'a str, chars: usize) -> Self {
        Self { hex, limit: Some(chars) }
    }

    fn is_ucs2(&self) -> bool {
        !self.hex.is_empty() && self.hex.len() % 4 == 0 && self.hex.bytes().all(|b| b.is_ascii_hexdigit())
    }

    /// Valeurs 16 bits du texte hexadecimal.
    fn units(&self) -> impl Iterator<Item = u16> + '_ {
        self.hex
            .as_bytes()
            .chunks(4)
            .filter_map(|chunk| core::str::from_utf8(chunk).ok())
            .filter_map(|digits| u16::from_str_radix(digits, 16).ok())
    }
}

impl core::fmt::Display for Decoded<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write;
        if !self.is_ucs2() {
            return f.write_str(self.hex);
        }
        let limit = self.limit.unwrap_or(usize::MAX);
        let mut count = 0;
        let mut put = |f: &mut core::fmt::Formatter<'_>, c: char| -> core::fmt::Result {
            if count == limit {
                f.write_str("...")?;
            }
            count += 1;
            if count > limit {
                return Ok(());
            }
            match c {
                // Apercu sur une ligne : retours a la ligne remplaces.
                '\n' | '\r' if self.limit.is_some() => f.write_char(' '),
                c => f.write_char(c),
            }
        };

        if self.units().all(|unit| unit < 0x80) {
            // Codes GSM bruts (cf. commentaire de la structure).
            let mut escape = false;
            for unit in self.units() {
                let code = unit as u8;
                if escape {
                    escape = false;
                    put(f, gsm_extension(code))?;
                } else if code == 0x1B {
                    escape = true;
                } else {
                    put(f, GSM_BASIC[code as usize])?;
                }
            }
        } else {
            for decoded in char::decode_utf16(self.units()) {
                put(f, decoded.unwrap_or(char::REPLACEMENT_CHARACTER))?;
            }
        }
        Ok(())
    }
}

/// Alphabet GSM 03.38 (table de base) : code GSM 0x00..0x7F -> caractere.
/// 0x1B (echappement vers la table etendue) est traite a part.
#[rustfmt::skip]
const GSM_BASIC: [char; 128] = [
    '@', '£', '$', '¥', 'è', 'é', 'ù', 'ì', 'ò', 'Ç', '\n', 'Ø', 'ø', '\r', 'Å', 'å',
    'Δ', '_', 'Φ', 'Γ', 'Λ', 'Ω', 'Π', 'Ψ', 'Σ', 'Θ', 'Ξ', ' ', 'Æ', 'æ', 'ß', 'É',
    ' ', '!', '"', '#', '¤', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?',
    '¡', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O',
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'Ä', 'Ö', 'Ñ', 'Ü', '§',
    '¿', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ä', 'ö', 'ñ', 'ü', 'à',
];

/// Table etendue GSM 03.38 : caractere apres l'echappement 0x1B.
fn gsm_extension(code: u8) -> char {
    match code {
        0x0A => '\n', // saut de page
        0x14 => '^',
        0x28 => '{',
        0x29 => '}',
        0x2F => '\\',
        0x3C => '[',
        0x3D => '~',
        0x3E => ']',
        0x40 => '|',
        0x65 => '€',
        _ => '?',
    }
}

/// Extrait l'heure de `"yy/MM/dd,hh:mm:ss+zz"` (reponse a AT+CCLK? ; heure
/// locale, le fuseau zz en quarts d'heure est ignore). `None` si l'annee n'est
/// pas entre 2025 et 2069 : le module renvoie "70/01/01" tant qu'il n'a pas
/// recu l'heure.
fn parse_cclk(text: &str) -> Option<DateTime> {
    let text = text.trim_start_matches('"').as_bytes().get(..17)?; // "yy/MM/dd,hh:mm:ss"
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
