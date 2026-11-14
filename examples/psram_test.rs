//! Test de la PSRAM du module ESP32-WROVER (RAM externe en SPI, dans le module).
//!
//! 1. Initialise la PSRAM et affiche la taille detectee (l'ESP32 ne peut en
//!    adresser directement que 4 Mo, meme si la puce en contient 8).
//! 2. Ecrit sur toute la zone un motif qui depend de l'adresse, relit et compte
//!    les erreurs ; puis recommence avec le motif inverse (chaque bit est ainsi
//!    teste a 0 et a 1).
//! 3. Affiche les debits d'ecriture et de lecture.
//!
//!     cargo run --release --example psram_test
//!
//! (--release recommande par esp-hal pour la PSRAM.)

#![no_std]
#![no_main]

use esp32_quick::board_console;
use esp_hal::main;
use esp_hal::psram::{Psram, PsramConfig};
use esp_hal::time::Instant;

esp_bootloader_esp_idf::esp_app_desc!();

/// Motif dependant de l'adresse : une erreur d'adressage (deux adresses qui
/// tombent sur la meme cellule) donne une mauvaise valeur a la relecture.
fn pattern(index: usize, inverted: bool) -> u32 {
    let value = (index as u32).wrapping_mul(0x9E37_79B9) ^ 0xA5A5_A5A5;
    if inverted { !value } else { value }
}

/// Ecrit puis relit toute la zone ; renvoie (erreurs, ms ecriture, ms lecture).
fn pass(words: *mut u32, count: usize, inverted: bool) -> (usize, u64, u64) {
    let start = Instant::now();
    for i in 0..count {
        // SAFETY : i < count, zone fournie par Psram::raw_parts.
        unsafe { words.add(i).write_volatile(pattern(i, inverted)) };
    }
    let write_ms = (Instant::now() - start).as_millis();

    let start = Instant::now();
    let mut errors = 0usize;
    for i in 0..count {
        // SAFETY : idem.
        let read = unsafe { words.add(i).read_volatile() };
        if read != pattern(i, inverted) {
            if errors < 5 {
                esp_println::println!(
                    "  erreur a +0x{:06X} : lu 0x{:08X}, attendu 0x{:08X}",
                    i * 4,
                    read,
                    pattern(i, inverted)
                );
            }
            errors += 1;
        }
    }
    let read_ms = (Instant::now() - start).as_millis();
    (errors, write_ms, read_ms)
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let _console = board_console!(peripherals); // avant tout message (cf. main.rs)

    esp_println::println!("== Test PSRAM ==");
    let psram = Psram::new(peripherals.PSRAM, PsramConfig::default());
    let (start, size) = psram.raw_parts();
    esp_println::println!("PSRAM : {} Ko adressables a {:p}", size / 1024, start);
    if size == 0 {
        esp_println::println!("ECHEC : PSRAM non detectee (module sans PSRAM, ou probleme materiel).");
        loop {}
    }

    let words = start as *mut u32;
    let count = size / 4;
    let mut total_errors = 0usize;
    for (label, inverted) in [("motif", false), ("motif inverse", true)] {
        let (errors, write_ms, read_ms) = pass(words, count, inverted);
        let mb = size as u64 * 1000 / (1024 * 1024); // Mo x 1000
        esp_println::println!(
            "{} : {} erreur(s) ; ecriture {} ms ({} Mo/s), lecture {} ms ({} Mo/s)",
            label,
            errors,
            write_ms,
            if write_ms > 0 { mb / write_ms } else { 0 },
            read_ms,
            if read_ms > 0 { mb / read_ms } else { 0 }
        );
        total_errors += errors;
    }

    if total_errors == 0 {
        esp_println::println!("== PSRAM OK : {} Ko testes, aucune erreur ==", size / 1024);
    } else {
        esp_println::println!("== PSRAM EN ECHEC : {} erreur(s) ==", total_errors);
    }
    loop {}
}
