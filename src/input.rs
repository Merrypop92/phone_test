//! Entrees utilisateur, independantes de leur source.
//!
//! Les apps recoivent des `Input`, pas du texte brut. Aujourd'hui toutes les
//! entrees viennent de la console serie (`Input::from_console_line`) ; la
//! matrice de boutons, puis l'ecran tactile, produiront les memes evenements
//! sans qu'il faille modifier les apps.

/// Touche de navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Back,
}

/// Une entree utilisateur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input<'a> {
    /// Une touche de navigation.
    Key(Key),
    /// Une ligne de texte saisie (commande ou contenu).
    Text(&'a str),
}

impl<'a> Input<'a> {
    /// Traduit une ligne tapee sur la console serie : les mots UP, DOWN, LEFT,
    /// RIGHT, ENTER et BACK (seuls sur la ligne) deviennent des touches, tout
    /// le reste est du texte.
    pub fn from_console_line(line: &'a str) -> Self {
        let key = match line.trim() {
            "UP" => Key::Up,
            "DOWN" => Key::Down,
            "LEFT" => Key::Left,
            "RIGHT" => Key::Right,
            "ENTER" => Key::Enter,
            "BACK" => Key::Back,
            _ => return Input::Text(line),
        };
        Input::Key(key)
    }
}

// ---------------------------------------------------------------------------
// Bouton physique avec anti-rebond
// ---------------------------------------------------------------------------

use esp_hal::gpio::Input as Pin;
use esp_hal::time::{Duration, Instant};

/// Duree pendant laquelle l'etat lu doit rester stable pour etre pris en
/// compte (un contact mecanique "rebondit" quelques millisecondes).
const DEBOUNCE: Duration = Duration::from_millis(30);

/// Changement d'etat d'un bouton.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonEvent {
    Pressed,
    /// Relache apres avoir ete appuye `held_ms` millisecondes.
    Released { held_ms: u64 },
}

/// Bouton relie a la masse, lu sur une broche avec pull-up : niveau bas =
/// appuye. Appeler `poll()` regulierement (toutes les ~20 ms).
pub struct Button {
    pin: Pin<'static>,
    /// Etat valide (apres anti-rebond).
    pressed: bool,
    /// Dernier etat lu, et depuis quand.
    raw: bool,
    raw_since: Instant,
    pressed_at: Instant,
}

impl Button {
    pub fn new(pin: Pin<'static>) -> Self {
        let now = Instant::now();
        let raw = pin.is_low();
        Self { pin, pressed: raw, raw, raw_since: now, pressed_at: now }
    }

    /// Etat valide du bouton.
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    /// Lit la broche ; renvoie un evenement quand l'etat change (apres
    /// anti-rebond).
    pub fn poll(&mut self) -> Option<ButtonEvent> {
        let now = Instant::now();
        let raw = self.pin.is_low();
        if raw != self.raw {
            self.raw = raw;
            self.raw_since = now;
            return None;
        }
        if raw == self.pressed || now - self.raw_since < DEBOUNCE {
            return None;
        }
        self.pressed = raw;
        if raw {
            self.pressed_at = now;
            Some(ButtonEvent::Pressed)
        } else {
            Some(ButtonEvent::Released { held_ms: (now - self.pressed_at).as_millis() })
        }
    }
}

