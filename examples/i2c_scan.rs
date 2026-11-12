#![no_std]
#![no_main]

// Diagnostic rapide du bus I2C de la matrice de boutons (TCA8418), pour
// verifier la soudure. Trois etapes, repetees toutes les 3 s (on peut appuyer
// sur la puce pendant le test : un resultat qui change = mauvais contact) :
//
//   1. Etat electrique de SDA/SCL avant toute communication :
//      - avec le pull-down interne de l'ESP32 : une ligne qui reste HAUTE
//        prouve qu'une resistance de pull-up externe est bien presente ;
//      - avec le pull-up interne : une ligne qui reste BASSE est court-circuitee
//        a la masse (pont de soudure, puce qui tire la ligne...).
//   2. Balayage de toutes les adresses I2C (0x08 a 0x77).
//   3. Si le TCA8418 (0x34) repond : ecriture puis relecture d'un registre.
//
// Cablage : voir src/board.rs (SDA=GPIO21, SCL=GPIO22, INT#=GPIO25).

use esp32_quick::board::{self, KEYPAD_I2C_ADDR};
use esp_hal::gpio::{Input, InputConfig, Pull};
use esp_hal::i2c::master::I2c;
use esp_hal::{Blocking, main};

esp_bootloader_esp_idf::esp_app_desc!();

/// Registre KP_GPIO1 du TCA8418 (lignes ROW0-7 en mode clavier), 0x00 au reset.
/// Utilise pour le test ecriture/relecture, puis remis a 0x00.
const REG_KP_GPIO1: u8 = 0x1D;
const REG_CFG: u8 = 0x01;
const REG_INT_STAT: u8 = 0x02;

fn level(input: &Input) -> &'static str {
    if input.is_high() { "HAUT" } else { "BAS " }
}

#[main]
fn main() -> ! {
    let mut p = esp_hal::init(esp_hal::Config::default());

    esp_println::println!("== Diagnostic I2C / TCA8418 ==");

    // --- 1. Etat electrique des lignes (avant d'initialiser l'I2C) ---
    {
        let down = InputConfig::default().with_pull(Pull::Down);
        let up = InputConfig::default().with_pull(Pull::Up);

        let sda = Input::new(p.GPIO21.reborrow(), down);
        let scl = Input::new(p.GPIO22.reborrow(), down);
        board::delay_ms(5);
        let (sda_down, scl_down) = (sda.is_high(), scl.is_high());
        esp_println::println!(
            "[1] Avec pull-down interne : SDA={} SCL={}",
            level(&sda),
            level(&scl)
        );
        drop((sda, scl));

        let sda = Input::new(p.GPIO21.reborrow(), up);
        let scl = Input::new(p.GPIO22.reborrow(), up);
        board::delay_ms(5);
        let (sda_up, scl_up) = (sda.is_high(), scl.is_high());
        esp_println::println!(
            "[1] Avec pull-up interne   : SDA={} SCL={}",
            level(&sda),
            level(&scl)
        );
        drop((sda, scl));

        for (name, with_down, with_up) in [("SDA", sda_down, sda_up), ("SCL", scl_down, scl_up)] {
            let verdict = match (with_down, with_up) {
                (true, true) => "OK : pull-up externe present",
                (false, true) => "PROBLEME : pas de pull-up externe (resistance absente ou mal soudee)",
                (false, false) => "PROBLEME : ligne tiree a la masse (court-circuit ou pont de soudure)",
                (true, false) => "BIZARRE : lecture incoherente, verifier le cablage",
            };
            esp_println::println!("    {name} -> {verdict}");
        }

        // R_9 est en serie (pas de pull-up externe) : pull-up interne obligatoire.
        let int = Input::new(p.GPIO25.reborrow(), up);
        board::delay_ms(5);
        esp_println::println!(
            "[1] INT# (GPIO25, pull-up interne) = {} (HAUT attendu au repos ; BAS = puce qui signale un evenement ou court-circuit)",
            level(&int)
        );
    }

    // --- 2 et 3. Initialisation I2C puis boucle de tests ---
    let (mut i2c, _int) = esp32_quick::board_keypad!(p);

    loop {
        esp_println::println!("--------------------------------------------");
        scan(&mut i2c);
        test_tca8418(&mut i2c);
        board::delay_ms(3000);
    }
}

/// Balaye les adresses 0x08..=0x77 et affiche celles qui repondent.
fn scan(i2c: &mut I2c<'_, Blocking>) {
    let mut found = 0;
    for address in 0x08u8..=0x77 {
        let mut byte = [0u8; 1];
        if i2c.read(address, &mut byte).is_ok() {
            found += 1;
            let note = if address == KEYPAD_I2C_ADDR { "  <- TCA8418" } else { "" };
            esp_println::println!("[2] Peripherique trouve a 0x{address:02X}{note}");
        }
    }
    if found == 0 {
        esp_println::println!("[2] Aucun peripherique ne repond sur le bus.");
    }
}

/// Lit quelques registres du TCA8418, puis ecrit et relit KP_GPIO1.
fn test_tca8418(i2c: &mut I2c<'_, Blocking>) {
    let addr = KEYPAD_I2C_ADDR;
    let mut read = |reg: u8| {
        let mut value = [0u8; 1];
        i2c.write_read(addr, &[reg], &mut value).map(|_| value[0])
    };

    match (read(REG_CFG), read(REG_INT_STAT)) {
        (Ok(cfg), Ok(int_stat)) => {
            esp_println::println!("[3] TCA8418 : CFG=0x{cfg:02X} INT_STAT=0x{int_stat:02X}");
        }
        (Err(e), _) | (_, Err(e)) => {
            esp_println::println!("[3] TCA8418 ne repond pas a 0x{addr:02X} : {e:?}");
            return;
        }
    }

    let write = i2c.write(addr, &[REG_KP_GPIO1, 0xA5]);
    let mut value = [0u8; 1];
    let readback = i2c.write_read(addr, &[REG_KP_GPIO1], &mut value);
    let _ = i2c.write(addr, &[REG_KP_GPIO1, 0x00]); // remet la valeur par defaut

    match (write, readback) {
        (Ok(()), Ok(())) if value[0] == 0xA5 => {
            esp_println::println!("[3] Ecriture/relecture OK (0xA5) : la puce fonctionne.");
        }
        (Ok(()), Ok(())) => esp_println::println!(
            "[3] Relecture incorrecte : 0x{:02X} au lieu de 0xA5 (communication instable ?)",
            value[0]
        ),
        (Err(e), _) | (_, Err(e)) => esp_println::println!("[3] Echec ecriture/relecture : {e:?}"),
    }
}
