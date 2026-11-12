#![no_std]
#![no_main]

mod app;
mod calculator_app;
mod menu_app;
mod message_app;
mod task_app;

use app::{App, Launcher};
use calculator_app::CalculatorApp;
use message_app::MessageApp;
use task_app::TaskApp;
use esp_hal::delay::Delay;
use esp_hal::main;

use esp32_quick::clock::{self, DateTime};
use esp32_quick::{Epd, board, board_console, board_display};

esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut delay = Delay::new();

    let mut serial = board_console!(peripherals);
    let mut epd = board_display!(peripherals);
    let buf = board::frame_buffer();

    epd.hw_init(&mut delay);

    // Registre des apps, dans l'ordre du menu. Pour ajouter une app : creer son
    // fichier, le declarer avec `mod` en haut de ce fichier, et l'ajouter ici.
    let registry: &mut [&mut dyn App] = &mut [
        &mut MessageApp::new(),
        &mut TaskApp::new(),
        &mut CalculatorApp::new(),
    ];
    let mut launcher = Launcher::new(registry);
    launcher.draw(&mut epd, buf);
    esp_println::println!("Menu pret : UP, DOWN, ENTER, BACK puis Entree.");
    esp_println::println!("Heure non reglee : TIME AAAA-MM-JJ HH:MM (ex. TIME 2026-11-12 14:30).");

    let mut line = [0u8; 96];
    let mut length = 0usize;
    let mut byte = [0u8; 1];

    loop {
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
                        handle_time_command(input[4..].trim(), &mut launcher, &mut epd, buf);
                        length = 0;
                        continue;
                    }

                    launcher.handle_input(input, &mut epd, buf);
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
fn handle_time_command(argument: &str, launcher: &mut Launcher, epd: &mut Epd, buf: &mut [u8]) {
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
            launcher.on_time_changed(epd, buf);
        }
        None => esp_println::println!(
            "Date invalide. Usage : TIME AAAA-MM-JJ HH:MM[:SS] (annees {} a {})",
            clock::MIN_YEAR,
            clock::MAX_YEAR
        ),
    }
}
