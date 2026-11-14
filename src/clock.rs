//! Heure et date du DocPhone.
//!
//! Source de l'heure : le reseau mobile (module LTE, cf. lte.rs), lue au
//! demarrage puis toutes les heures. En attendant, heure de compilation, ou
//! reglage a la main (commande serie `TIME`). On y ajoute le temps ecoule
//! depuis le reglage (compteur interne de l'ESP32, derive de quelques
//! secondes par jour, corrigee a chaque resynchronisation). Un RTC externe est
//! prevu sur une prochaine version du materiel.
//!
//! Pour brancher plus tard une autre source (RTC, GPS), il suffira d'appeler
//! `set()` avec l'heure lue : les apps n'utilisent que `now()`.
//!
//! Les calculs de dates sont regroupes ici (un seul endroit, contrairement a
//! PaxOS) : annees bissextiles, jours par mois, jour de la semaine (lundi en
//! premier), numero de semaine ISO 8601.

use core::cell::Cell;
use core::fmt;

use critical_section::Mutex;
use esp_hal::time::Instant;

/// Annees acceptees par `TIME` (bornes volontairement larges mais realistes).
pub const MIN_YEAR: u16 = 2000;
pub const MAX_YEAR: u16 = 2099;

pub const WEEKDAY_NAMES: [&str; 7] = ["Lundi", "Mardi", "Mercredi", "Jeudi", "Vendredi", "Samedi", "Dimanche"];
/// Abreviations de 3 lettres (affichage en palettes).
pub const WEEKDAY_SHORT: [&str; 7] = ["LUN", "MAR", "MER", "JEU", "VEN", "SAM", "DIM"];
pub const MONTH_SHORT: [&str; 12] = ["JAN", "FEV", "MAR", "AVR", "MAI", "JUN", "JUL", "AOU", "SEP", "OCT", "NOV", "DEC"];
pub const MONTH_NAMES: [&str; 12] = [
    "Janvier", "Fevrier", "Mars", "Avril", "Mai", "Juin",
    "Juillet", "Aout", "Septembre", "Octobre", "Novembre", "Decembre",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8, // 1..=12
    pub day: u8,   // 1..=31
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub fn is_leap_year(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Nombre de jours depuis le 1970-01-01 (algorithme "days_from_civil" de H. Hinnant).
fn days_from_civil(year: u16, month: u8, day: u8) -> i64 {
    let (m, d) = (month as i64, day as i64);
    let y = year as i64 - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse de `days_from_civil`.
fn civil_from_days(days: i64) -> (u16, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as u16;
    (year, month, day)
}

impl DateTime {
    /// Construit une date en verifiant chaque champ.
    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Option<Self> {
        let valid = (MIN_YEAR..=MAX_YEAR).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month)
            && hour < 24
            && minute < 60
            && second < 60;
        valid.then_some(Self { year, month, day, hour, minute, second })
    }

    /// Lit "AAAA-MM-JJ HH:MM" ou "AAAA-MM-JJ HH:MM:SS".
    pub fn parse(text: &str) -> Option<Self> {
        let (date, time) = text.trim().split_once(' ')?;

        let mut date_parts = date.split('-');
        let year = date_parts.next()?.parse().ok()?;
        let month = date_parts.next()?.parse().ok()?;
        let day = date_parts.next()?.parse().ok()?;
        if date_parts.next().is_some() {
            return None;
        }

        let mut time_parts = time.trim().split(':');
        let hour = time_parts.next()?.parse().ok()?;
        let minute = time_parts.next()?.parse().ok()?;
        let second = match time_parts.next() {
            Some(s) => s.parse().ok()?,
            None => 0,
        };
        if time_parts.next().is_some() {
            return None;
        }

        Self::new(year, month, day, hour, minute, second)
    }

    /// Secondes depuis le 1970-01-01 00:00:00 (sans fuseau horaire : heure locale).
    pub fn to_secs(&self) -> u64 {
        days_from_civil(self.year, self.month, self.day) as u64 * 86_400
            + self.hour as u64 * 3600
            + self.minute as u64 * 60
            + self.second as u64
    }

    pub fn from_secs(secs: u64) -> Self {
        let (year, month, day) = civil_from_days((secs / 86_400) as i64);
        let rest = secs % 86_400;
        Self {
            year,
            month,
            day,
            hour: (rest / 3600) as u8,
            minute: (rest % 3600 / 60) as u8,
            second: (rest % 60) as u8,
        }
    }

    /// Jour de la semaine, 0 = lundi ... 6 = dimanche (le 1970-01-01 etait un jeudi).
    pub fn weekday(&self) -> u8 {
        (days_from_civil(self.year, self.month, self.day) + 3).rem_euclid(7) as u8
    }

    pub fn weekday_name(&self) -> &'static str {
        WEEKDAY_NAMES[self.weekday() as usize]
    }

    pub fn month_name(&self) -> &'static str {
        MONTH_NAMES[self.month as usize - 1]
    }

    /// Jour de la semaine en 3 lettres ("JEU").
    pub fn weekday_short(&self) -> &'static str {
        WEEKDAY_SHORT[self.weekday() as usize]
    }

    /// Mois en 3 lettres ("NOV").
    pub fn month_short(&self) -> &'static str {
        MONTH_SHORT[self.month as usize - 1]
    }

    /// Numero de semaine ISO 8601 (la semaine 1 contient le premier jeudi de l'annee).
    pub fn iso_week(&self) -> u8 {
        let days = days_from_civil(self.year, self.month, self.day);
        let thursday = days - self.weekday() as i64 + 3;
        let (thursday_year, _, _) = civil_from_days(thursday);
        ((thursday - days_from_civil(thursday_year, 1, 1)) / 7 + 1) as u8
    }
}

/// Format "AAAA-MM-JJ HH:MM:SS".
impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// Reference posee par `set()` : (secondes depuis 1970, secondes depuis le demarrage).
static REFERENCE: Mutex<Cell<Option<(u64, u64)>>> = Mutex::new(Cell::new(None));

fn uptime_secs() -> u64 {
    Instant::now().duration_since_epoch().as_secs()
}

/// Millisecondes ecoulees depuis le demarrage (compteur monotone, toujours
/// disponible meme si l'heure n'est pas reglee). Pour chronometres et minuteurs.
pub fn uptime_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

/// Heure locale du PC au moment de la compilation (fournie par build.rs).
pub const BUILD_TIME: &str = env!("DOCPHONE_BUILD_TIME");

/// Regle l'horloge sur l'heure de compilation (solution de depannage tant
/// qu'il n'y a pas de RTC). Renvoie l'heure posee.
pub fn set_to_build_time() -> Option<DateTime> {
    let build_time = DateTime::parse(BUILD_TIME)?;
    set(build_time);
    Some(build_time)
}

/// Regle l'heure courante.
pub fn set(now: DateTime) {
    let reference = (now.to_secs(), uptime_secs());
    critical_section::with(|cs| REFERENCE.borrow(cs).set(Some(reference)));
}

/// Heure courante, ou `None` si elle n'a jamais ete reglee depuis le demarrage.
pub fn now() -> Option<DateTime> {
    let (base_secs, base_uptime) = critical_section::with(|cs| REFERENCE.borrow(cs).get())?;
    Some(DateTime::from_secs(base_secs + (uptime_secs() - base_uptime)))
}
