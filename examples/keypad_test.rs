#![no_std]
#![no_main]

// Test de la matrice de boutons (TCA8418, controleur clavier I2C).
// Registres/logique verifies contre la datasheet TI (TCA8418.pdf) et le driver
// Linux kernel (drivers/input/keyboard/tca8418_keypad.c) plutot que de deviner.
//
// RESET# du TCA8418 est cable directement sur 3V3 (pas de GPIO) : la puce est
// donc toujours hors reset, pas besoin de la gerer ici.

use esp32_quick::board::{self, KEYPAD_I2C_ADDR as TCA8418_ADDR};
use esp32_quick::board_keypad;
use esp_hal::main;

esp_bootloader_esp_idf::esp_app_desc!();

const REG_INT_STAT: u8 = 0x02;
const REG_KEY_EVENT_A: u8 = 0x04;
const REG_KP_GPIO1: u8 = 0x1D;
const REG_KP_GPIO2: u8 = 0x1E;
const REG_KP_GPIO3: u8 = 0x1F;

const MAX_COLS: u8 = 10;

/// Decode un octet KEY_EVENT_A en (pressed, row, col).
/// Formule reprise telle quelle du driver kernel TCA8418 (le decalage sur col==0
/// n'est pas une erreur, c'est la convention d'encodage du chip).
fn decode_event(reg: u8) -> (bool, i32, i32) {
    let pressed = reg & 0x80 != 0;
    let code = reg & 0x7F;
    let mut row = (code / MAX_COLS) as i32;
    let mut col = (code % MAX_COLS) as i32;
    if col == 0 {
        row -= 1;
        col = MAX_COLS as i32 - 1;
    } else {
        col -= 1;
    }
    (pressed, row, col)
}

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default();
    let peripherals = esp_hal::init(config);

    esp_println::println!("== Test matrice de boutons (TCA8418) ==");

    // Cablage (I2C0 SDA=GPIO21 SCL=GPIO22, INT#=GPIO25) : voir src/board.rs.
    let (mut i2c, kdb_int) = board_keypad!(peripherals);

    // Active les 8 lignes (ROW0-7) et les 10 colonnes (COL0-9) comme matrice clavier.
    let mut ok = true;
    ok &= i2c.write(TCA8418_ADDR, &[REG_KP_GPIO1, 0xFF]).is_ok();
    ok &= i2c.write(TCA8418_ADDR, &[REG_KP_GPIO2, 0xFF]).is_ok();
    ok &= i2c.write(TCA8418_ADDR, &[REG_KP_GPIO3, 0x03]).is_ok();

    if !ok {
        esp_println::println!(
            "echec de config du TCA8418 (pas de reponse I2C ?) -> verifier soudure U11 / pull-ups SDA-SCL"
        );
    } else {
        esp_println::println!("TCA8418 configure OK (8 lignes x 10 colonnes). Appuie sur les boutons...");
    }

    loop {
        // Lecture de INT_STAT juste pour diagnostic (pas indispensable en polling pur).
        let mut int_stat = [0u8; 1];
        let int_stat_ok = i2c.write_read(TCA8418_ADDR, &[REG_INT_STAT], &mut int_stat).is_ok();

        // Vide le FIFO d'evenements (chaque lecture de KEY_EVENT_A depile un evenement,
        // reg==0 signifie FIFO vide -> meme logique que le driver Linux).
        loop {
            let mut reg = [0u8; 1];
            match i2c.write_read(TCA8418_ADDR, &[REG_KEY_EVENT_A], &mut reg) {
                Ok(()) if reg[0] != 0 => {
                    let (pressed, row, col) = decode_event(reg[0]);
                    esp_println::println!(
                        "bouton: ROW{row} COL{col} -> {}",
                        if pressed { "appuye" } else { "relache" }
                    );
                }
                Ok(()) => break, // FIFO vide
                Err(e) => {
                    esp_println::println!("erreur lecture KEY_EVENT_A: {e:?}");
                    break;
                }
            }
        }

        // Nettoie les flags d'interruption (utile meme sans exploiter reellement le pin INT).
        if int_stat_ok && int_stat[0] != 0 {
            let _ = i2c.write(TCA8418_ADDR, &[REG_INT_STAT, 0xFF]);
        }

        // Etat brut du pin INT#, pour verifier le cablage meme sans logique d'IRQ.
        if kdb_int.is_low() {
            esp_println::println!("KDB_INT (GPIO25) = LOW (evenement en attente)");
        }

        board::delay_ms(50);
    }
}
