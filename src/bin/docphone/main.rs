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
use esp32_quick::lte::{Lte, Request};
use esp32_quick::{board, board_console, board_display, board_led, board_lte, board_watchdog, system};

esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    // Le plus tot possible : lit la trace de panic et active le detecteur de
    // brownout (sans ca, un brownout n'est pas detecte, cf. system.rs).
    system::init();
    let (bod_on, bod_reset, bod_level) = system::brownout_status();
    esp_println::println!(
        "Detecteur de brownout : actif={} reset={} seuil={}",
        bod_on, bod_reset, bod_level
    );

    let abnormal = system::last_reset_abnormal();
    esp_println::println!(
        "Demarrage. Dernier reset : {} ({:?}){}",
        system::last_reset_label(),
        system::reset_reason(),
        if abnormal { "  <-- ANORMAL" } else { "" }
    );

    // Signe de vie sans PC : 2 clignotements courts, 5 si le reset est anormal.
    let mut led = board_led!(peripherals);
    board::blink(&mut led, if abnormal { 5 } else { 2 }, 100);

    // Redemarre la carte si la boucle principale se bloque plus de 10 s.
    let mut watchdog = board_watchdog!(peripherals);

    let mut serial = board_console!(peripherals);
    // Module LTE : allumage et heure reseau, en tache de fond (cf. lte.rs).
    let (lte_uart, lte_pwrkey) = board_lte!(peripherals);
    let mut lte = Lte::new(lte_uart, lte_pwrkey);
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
    // Court pour que les mises a jour (bascule des palettes) demarrent toujours
    // juste apres le changement de seconde ; un tick sans changement ne coute
    // presque rien.
    const TICK_MS: u64 = 20;
    let mut next_tick = 0u64;

    let mut line = [0u8; 96];
    let mut length = 0usize;
    let mut byte = [0u8; 1];

    loop {
        let now = clock::uptime_ms();
        if now >= next_tick {
            watchdog.feed();
            launcher.tick();
            next_tick = now + TICK_MS;
        }

        // L'heure reseau remplace l'heure de compilation (ou celle de TIME) ;
        // le menu et l'horloge se mettent a jour d'eux-memes au tick suivant.
        if let Some(network_time) = lte.poll() {
            clock::set(network_time);
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

                    // SMS : commandes globales, affichage sur la console (cf. lte.rs).
                    if input.eq_ignore_ascii_case("SMS")
                        || input.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("SMS "))
                    {
                        handle_sms_command(&mut lte, input[3..].trim());
                        length = 0;
                        continue;
                    }

                    // Commandes de test des plantages (cf. system.rs, board.rs).
                    if input == "TEST PANIC" {
                        panic!("panic volontaire (commande TEST PANIC)");
                    }
                    if input == "TEST HANG" {
                        esp_println::println!(
                            "Blocage volontaire : le watchdog doit redemarrer la carte dans {} s.",
                            board::WATCHDOG_TIMEOUT_S
                        );
                        #[allow(clippy::empty_loop)]
                        loop {}
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

/// `SMS` liste les SMS, `SMS LIRE <n>` en affiche un, `SMS SUPPR <n>` le
/// supprime. Les reponses s'affichent quand le module a repondu.
fn handle_sms_command(lte: &mut Lte, argument: &str) {
    let mut words = argument.split_whitespace();
    let action = words.next().unwrap_or("");
    let number = words.next().and_then(|n| n.parse::<u16>().ok());
    let request = if action.is_empty() {
        Some(Request::ListSms)
    } else if action.eq_ignore_ascii_case("LIRE") {
        number.map(Request::ReadSms)
    } else if action.eq_ignore_ascii_case("SUPPR") {
        number.map(Request::DeleteSms)
    } else {
        None
    };
    match request {
        Some(request) => {
            let _ = lte.request(request);
        }
        None => esp_println::println!("Usage : SMS | SMS LIRE <n> | SMS SUPPR <n>"),
    }
}
