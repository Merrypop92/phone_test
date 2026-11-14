//! Scan Bluetooth Low Energy : valide la partie Bluetooth de la radio de
//! l'ESP32 (meme antenne u.FL que le WiFi).
//!
//! Pas de pile Bluetooth complete : on parle directement au controleur BLE en
//! HCI (le protocole standard entre une pile Bluetooth et la puce radio).
//! Trames au format H4 : 1er octet = type (0x01 commande, 0x04 evenement).
//!   1. HCI_Reset
//!   2. HCI_Set_Event_Mask (active les evenements "LE Meta", dont les annonces)
//!   3. LE_Set_Scan_Parameters (scan actif) puis LE_Set_Scan_Enable
//!   4. Pendant SCAN_SECONDS : on lit les "LE Advertising Report" (annonces
//!      des appareils autour : adresse, signal, nom s'il est annonce)
//!   5. Arret du scan, tableau des appareils du plus fort au plus faible.
//!
//!     cargo run --release --example ble_scan

#![no_std]
#![no_main]

use esp32_quick::{board_console, system};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::main;
use esp_hal::time::{Duration, Instant};
use esp_hal::timer::timg::TimerGroup;
use esp_radio::ble::controller::BleConnector;
use heapless::{String, Vec};

esp_bootloader_esp_idf::esp_app_desc!();

/// Duree du scan.
const SCAN_SECONDS: u64 = 10;
/// Appareils retenus / affiches.
const MAX_DEVICES: usize = 48;
const SHOWN: usize = 15;

/// Un appareil vu pendant le scan.
struct Device {
    address: [u8; 6],
    best_rssi: i8,
    adverts: u16,
    name: String<24>,
}

/// Envoie une commande HCI (opcode + parametres) et attend son "Command
/// Complete" (evenement 0x0E) ; renvoie le statut (0 = succes).
fn command(ble: &mut BleConnector<'_>, opcode: u16, params: &[u8]) -> Option<u8> {
    let mut packet: Vec<u8, 64> = Vec::new();
    let _ = packet.extend_from_slice(&[0x01, opcode as u8, (opcode >> 8) as u8, params.len() as u8]);
    let _ = packet.extend_from_slice(params);
    let _ = ble.write(&packet);

    let deadline = Instant::now() + Duration::from_millis(1000);
    let mut buffer = [0u8; 300];
    while Instant::now() < deadline {
        let length = ble.next(&mut buffer).unwrap_or(0);
        // 04 0E <len> <ncmd> <opcode lo> <opcode hi> <statut> ...
        if length >= 7 && buffer[0] == 0x04 && buffer[1] == 0x0E {
            let done = u16::from_le_bytes([buffer[4], buffer[5]]);
            if done == opcode {
                return Some(buffer[6]);
            }
        }
    }
    None
}

/// Nom annonce dans les donnees d'annonce (champs AD : longueur, type, donnees ;
/// type 0x09 = nom complet, 0x08 = nom raccourci).
fn advertised_name(data: &[u8]) -> Option<&str> {
    let mut rest = data;
    while rest.len() >= 2 {
        let length = rest[0] as usize;
        if length == 0 || length + 1 > rest.len() {
            break;
        }
        let (kind, value) = (rest[1], &rest[2..length + 1]);
        if kind == 0x09 || kind == 0x08 {
            return core::str::from_utf8(value).ok();
        }
        rest = &rest[length + 1..];
    }
    None
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let _console = board_console!(peripherals); // avant tout message

    system::init(); // brownout surveille pendant le test
    esp_println::println!(
        "== Scan BLE == (dernier reset : {}{})",
        system::last_reset_label(),
        if system::last_reset_abnormal() { "  <-- ANORMAL" } else { "" }
    );

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 36 * 1024);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);

    let mut ble = match BleConnector::new(peripherals.BT, Default::default()) {
        Ok(ble) => ble,
        Err(error) => {
            esp_println::println!("ECHEC : initialisation BLE : {:?}", error);
            loop {}
        }
    };

    let steps: [(&str, u16, &[u8]); 4] = [
        ("HCI_Reset", 0x0C03, &[]),
        // Masque d'evenements : tout, y compris le bit 61 (LE Meta).
        ("Set_Event_Mask", 0x0C01, &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x3F]),
        // Scan actif, intervalle 60 ms, fenetre 30 ms (unite 0,625 ms),
        // adresse publique, pas de filtre.
        ("LE_Set_Scan_Parameters", 0x200B, &[0x01, 0x60, 0x00, 0x30, 0x00, 0x00, 0x00]),
        // Scan actif, sans filtrage des doublons (on compte les annonces).
        ("LE_Set_Scan_Enable", 0x200C, &[0x01, 0x00]),
    ];
    for (name, opcode, params) in steps {
        match command(&mut ble, opcode, params) {
            Some(0) => esp_println::println!("{} : OK", name),
            Some(status) => {
                esp_println::println!("{} : erreur HCI 0x{:02X}", name, status);
                loop {}
            }
            None => {
                esp_println::println!("{} : pas de reponse du controleur", name);
                loop {}
            }
        }
    }
    esp_println::println!("Scan pendant {} s...", SCAN_SECONDS);

    let mut devices: Vec<Device, MAX_DEVICES> = Vec::new();
    let mut reports = 0u32;
    let mut buffer = [0u8; 300];
    let end = Instant::now() + Duration::from_secs(SCAN_SECONDS);
    while Instant::now() < end {
        let length = ble.next(&mut buffer).unwrap_or(0);
        // 04 3E <len> 02 <nb> <type> <type adr> <adr x6> <len data> <data...> <rssi>
        if length < 14 || buffer[0] != 0x04 || buffer[1] != 0x3E || buffer[3] != 0x02 || buffer[4] != 1 {
            continue;
        }
        let mut address = [0u8; 6];
        address.copy_from_slice(&buffer[7..13]);
        let data_length = buffer[13] as usize;
        if length < 14 + data_length + 1 {
            continue;
        }
        let data = &buffer[14..14 + data_length];
        let rssi = buffer[14 + data_length] as i8;
        reports += 1;

        let index = match devices.iter().position(|d| d.address == address) {
            Some(index) => index,
            None => {
                let device = Device { address, best_rssi: rssi, adverts: 0, name: String::new() };
                if devices.push(device).is_err() {
                    continue; // tableau plein
                }
                devices.len() - 1
            }
        };
        let device = &mut devices[index];
        device.adverts = device.adverts.saturating_add(1);
        device.best_rssi = device.best_rssi.max(rssi);
        if device.name.is_empty() {
            if let Some(name) = advertised_name(data) {
                for c in name.chars() {
                    if device.name.push(c).is_err() {
                        break;
                    }
                }
            }
        }
    }

    let _ = command(&mut ble, 0x200C, &[0x00, 0x00]); // arret du scan

    devices.sort_unstable_by(|a, b| b.best_rssi.cmp(&a.best_rssi));
    esp_println::println!();
    esp_println::println!("{} annonce(s), {} appareil(s), du plus fort au plus faible :", reports, devices.len());
    esp_println::println!("  dBm  annonces  adresse            nom");
    for device in devices.iter().take(SHOWN) {
        let a = device.address; // octets recus du poids faible au poids fort
        esp_println::println!(
            "  {:4}  {:8}  {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}  {}",
            device.best_rssi,
            device.adverts,
            a[5], a[4], a[3], a[2], a[1], a[0],
            if device.name.is_empty() { "-" } else { device.name.as_str() }
        );
    }
    if devices.len() > SHOWN {
        esp_println::println!("  ... et {} autre(s)", devices.len() - SHOWN);
    }
    esp_println::println!();
    esp_println::println!("== Fin du scan BLE, aucun redemarrage ==");
    loop {}
}
