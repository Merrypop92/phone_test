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
