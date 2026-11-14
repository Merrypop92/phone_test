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
//! avec `request()`).
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
/// Longueur affichee du texte dans la liste des SMS.
const LIST_PREVIEW_CHARS: usize = 50;

const CONFIGURE: [&str; 5] = [
    "ATE0",                // pas d'echo des commandes
    "AT+CTZU=1",           // heure reseau automatique (NITZ)
    "AT+CMGF=1",           // SMS en mode texte
    "AT+CSCS=\"UCS2\"",    // textes et numeros en UTF-16 hexadecimal
    "AT+CNMI=2,1,0,0,0",   // signaler les nouveaux SMS par +CMTI
];
const REATTACH: [&str; 2] = ["AT+CFUN=0", "AT+CFUN=1"];

/// Demande a traiter quand le module est pret (commandes SMS de la console).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Lister tous les SMS stockes (apercu du texte).
    ListSms,
    /// Afficher le SMS numero n en entier.
    ReadSms(u16),
    /// Supprimer le SMS numero n.
    DeleteSms(u16),
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
        let mut command = String::<24>::new();
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
        };
        self.send(&command, SMS_TIMEOUT);
    }

    fn finish_request(&mut self, request: Request) {
        let failed = !self.got_ok;
        if request == Request::ListSms && failed && self.got_error && !self.list_plain {
            // Forme UCS2 refusee : on retente tout de suite avec "ALL" en clair.
            self.list_plain = true;
            let _ = self.requests.push_front(Request::ListSms);
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
            _ if failed => esp_println::println!("LTE : le module a refuse la commande."),
            _ => {}
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
        self.body = Body::None;
        let _ = self.uart.write(command.as_bytes());
        let _ = self.uart.write(b"\r\n");
        self.deadline = Some(Instant::now() + timeout);
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
        // Autres messages spontanes (+CTZV, SMS DONE, +CGEV...) : ignores.
    }
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
