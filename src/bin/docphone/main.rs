#![no_std]
#![no_main]

mod app;
mod calculator_app;
mod clock_app;
mod menu_app;
mod message_app;
mod task_app;

use app::{App, Launcher};
use calculator_app::CalculatorApp;
use clock_app::ClockApp;
use message_app::MessageApp;
use task_app::TaskApp;
use esp_hal::main;

use esp32_quick::clock::{self, DateTime};
use esp32_quick::display::Display;
use esp32_quick::input::Input;
use esp32_quick::{board, board_console, board_display};

esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let mut serial = board_console!(peripherals);
    // L'ecran est initialise au premier affichage (rafraichissement complet).
    let display = Display::new(board_display!(peripherals), board::frame_buffer());

    match clock::set_to_build_time() {
        Some(time) => esp_println::println!(
            "Heure initialisee a l'heure de compilation : {} (TIME AAAA-MM-JJ HH:MM pour corriger).",
            time
        ),
        None => esp_println::println!("Heure non reglee : TIME AAAA-MM-JJ HH:MM (ex. TIME 2026-11-12 14:30)."),
    }

    // Registre des apps, dans l'ordre du menu. Pour ajouter une app : creer son
    // fichier, le declarer avec `mod` en haut de ce fichier, et l'ajouter ici.
    let registry: &mut [&mut dyn App] = &mut [
        &mut MessageApp::new(),
        &mut TaskApp::new(),
        &mut CalculatorApp::new(),
        &mut ClockApp::new(),
    ];
    let mut launcher = Launcher::new(registry, display);
    esp_println::println!("Menu pret : UP, DOWN, ENTER, BACK puis Entree.");

    // Intervalle entre deux appels a launcher.tick() (horloge, minuteur...).
    const TICK_MS: u64 = 200;
    let mut next_tick = 0u64;

    let mut line = [0u8; 96];
    let mut length = 0usize;
    let mut byte = [0u8; 1];

    loop {
        let now = clock::uptime_ms();
        if now >= next_tick {
            launcher.tick();
            next_tick = now + TICK_MS;
        }

        if !serial.read_ready() {
            continue;
        }

        match serial.read(&mut byte) {
            Ok(1) if byte[0] == b'\r' || byte[0] == b'\n' => {
                if length > 0 {
                    let input = core::str::from_utf8(&line[..length])
                        .unwrap_or("Texte non UTF-8");

                    // TIME est une commande globale, valable dans toutes les apps.
                    if input == "TIME" || input.starts_with("TIME ") {
                        handle_time_command(input[4..].trim());
                        length = 0;
                        continue;
                    }

                    launcher.handle_input(Input::from_console_line(input));
                    length = 0;
                }
            }
            Ok(1) if length < line.len() => {
                line[length] = byte[0];
                length += 1;
            }
            Ok(1) => {
                esp_println::println!("Ligne trop longue");
                length = 0;
            }
            Ok(_) | Err(_) => {}
        }
    }
}
/// `TIME` affiche l'heure ; `TIME AAAA-MM-JJ HH:MM[:SS]` la regle.
/// (Le menu et l'horloge se mettent a jour d'eux-memes au tick suivant.)
fn handle_time_command(argument: &str) {
    if argument.is_empty() {
        match clock::now() {
            Some(now) => esp_println::println!(
                "{} {} (semaine {})",
                now.weekday_name(),
                now,
                now.iso_week()
            ),
            None => esp_println::println!("Heure non reglee. Usage : TIME AAAA-MM-JJ HH:MM[:SS]"),
        }
        return;
    }

    match DateTime::parse(argument) {
        Some(now) => {
            clock::set(now);
            esp_println::println!("Heure reglee : {} {}", now.weekday_name(), now);
        }
        None => esp_println::println!(
            "Date invalide. Usage : TIME AAAA-MM-JJ HH:MM[:SS] (annees {} a {})",
            clock::MIN_YEAR,
            clock::MAX_YEAR
        ),
    }
}
