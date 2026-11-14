//! Console AT pour le module LTE A7682E.
//!
//! 1. Allume le module (impulsion PWRKEY) s'il ne repond pas deja a "AT".
//! 2. Lance un diagnostic SIM (etat, detection a chaud, identifiants).
//! 3. Passe en mode console : chaque ligne tapee dans le moniteur serie est
//!    envoyee au module, et tout ce que le module envoie est affiche.
//!
//!     cargo run --example lte_at_console
//!
//! Batterie branchee obligatoire : en emission le module tire des pics de ~2 A
//! sur VBATT.

#![no_std]
#![no_main]

use esp32_quick::board::delay_ms;
use esp32_quick::{board_console, board_lte};
use esp_hal::Blocking;
use esp_hal::gpio::Level;
use esp_hal::main;
use esp_hal::time::{Duration, Instant};
use esp_hal::uart::Uart;

esp_bootloader_esp_idf::esp_app_desc!();

/// Envoie une commande AT et affiche la reponse. S'arrete des que la reponse
/// contient OK ou ERROR, ou apres `timeout_ms`. Renvoie vrai si OK recu.
fn at(lte: &mut Uart<'static, Blocking>, command: &str, timeout_ms: u64) -> bool {
    esp_println::println!(">> {}", command);
    let _ = lte.write(command.as_bytes());
    let _ = lte.write(b"\r\n");
    let _ = lte.flush();

    let mut response = [0u8; 512];
    let mut length = 0usize;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if lte.read_ready() && length < response.len() {
            if let Ok(n) = lte.read(&mut response[length..]) {
                length += n;
            }
            // Fin de reponse : OK ou ERROR, et la ligne terminee (sinon
            // "+CME ERROR: SIM not inserted" serait coupe apres "ERROR").
            let text = &response[..length];
            if text.ends_with(b"\r\n") && (contains(text, b"OK\r\n") || contains(text, b"ERROR")) {
                break;
            }
        }
    }
    let text = core::str::from_utf8(&response[..length]).unwrap_or("(reponse non UTF-8)");
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && *l != command) {
        esp_println::println!("<< {}", line);
    }
    if length == 0 {
        esp_println::println!("<< (rien)");
    }
    contains(&response[..length], b"OK\r\n")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut console = board_console!(peripherals);
    let (mut lte, mut pwrkey) = board_lte!(peripherals);

    // --- 1. Allumage ---
    // Le module est sur VBATT : il reste allume quand l'ESP32 redemarre. Une
    // impulsion PWRKEY < 2,5 s ne l'eteint pas, mais on l'evite si inutile.
    esp_println::println!("== Module LTE : deja allume ? ==");
    let mut alive = at(&mut lte, "AT", 500);
    while !alive {
        esp_println::println!("== Impulsion PWRKEY (800 ms), demarrage du module (~10 s) ==");
        pwrkey.set_level(Level::High); // Q5 (2N7002) tire PWRKEY du module a la masse
        delay_ms(800);
        pwrkey.set_level(Level::Low);
        for _ in 0..15 {
            delay_ms(1000);
            if at(&mut lte, "AT", 500) {
                alive = true;
                break;
            }
        }
    }
    let _ = at(&mut lte, "ATE0", 500); // pas d'echo : reponses plus lisibles

    // --- 2. Diagnostic SIM ---
    esp_println::println!("== Diagnostic SIM ==");
    let _ = at(&mut lte, "ATI", 1000); // modele et version du firmware
    let _ = at(&mut lte, "AT+CFUN?", 1000); // 1 = radio et SIM actives
    let _ = at(&mut lte, "AT+UIMHOTSWAPON?", 1000); // 1 = detection a chaud (broche SIM_DET)
    let _ = at(&mut lte, "AT+UIMHOTSWAPLEVEL?", 1000); // niveau "SIM presente" sur SIM_DET
    let _ = at(&mut lte, "AT+CPIN?", 5000); // READY, SIM PIN, ou erreur
    let _ = at(&mut lte, "AT+CICCID", 2000); // numero de la carte SIM
    let _ = at(&mut lte, "AT+CSQ", 1000); // signal : 99 = inconnu

    esp_println::println!();
    esp_println::println!("== Console AT : tape une commande puis Entree ==");
    esp_println::println!("   Si SIM not inserted alors que SIM_DET n'est pas cablee :");
    esp_println::println!("   AT+UIMHOTSWAPON=0  puis  AT+CRESET  puis  AT+CPIN?");

    // --- 3. Console ---
    let mut line = [0u8; 128];
    let mut length = 0usize;
    let mut byte = [0u8; 1];
    loop {
        // Module -> PC : messages spontanes (+CPIN: READY, +CTZV, SMS...).
        if lte.read_ready() {
            let mut buffer = [0u8; 128];
            if let Ok(n) = lte.read(&mut buffer) {
                if let Ok(text) = core::str::from_utf8(&buffer[..n]) {
                    esp_println::print!("{}", text);
                }
            }
        }

        // PC -> module, ligne par ligne.
        if console.read_ready() {
            if let Ok(1) = console.read(&mut byte) {
                match byte[0] {
                    b'\r' | b'\n' => {
                        if length > 0 {
                            if let Ok(command) = core::str::from_utf8(&line[..length]) {
                                let _ = at(&mut lte, command.trim(), 10_000);
                            }
                            length = 0;
                        }
                    }
                    b if length < line.len() => {
                        line[length] = b;
                        length += 1;
                    }
                    _ => length = 0,
                }
            }
        }
    }
}
