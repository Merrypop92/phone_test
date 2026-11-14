//! Scan WiFi : valide la radio et l'antenne de l'ESP32, et la tenue du rail
//! 3V3 (TPS63025) quand la radio emet (pics de ~300-400 mA).
//!
//! Le detecteur de brownout est active (system::init) : si le 3V3 s'effondre
//! pendant un scan, la carte redemarre et affiche "Dernier reset : BROWNOUT".
//!
//! 10 scans actifs a la suite (la carte envoie des requetes sur chaque
//! canal), puis affichage des reseaux vus (cumul des scans), du plus fort au
//! plus faible.
//!
//! Le module est un ESP32-WROVER-IE : PAS d'antenne integree, seulement un
//! connecteur u.FL (IPEX MHF1). Sans antenne 2,4 GHz branchee, la reception
//! est quasi nulle (constate : 0 a 3 reseaux par scan).
//!
//!     cargo run --release --example wifi_scan

#![no_std]
#![no_main]

extern crate alloc;

use esp32_quick::{board_console, system};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::main;
use esp_hal::time::{Duration, Instant};
use esp_hal::timer::timg::TimerGroup;
use esp_radio::wifi::AuthenticationMethod;
use esp_radio::wifi::ap::AccessPointInfo;
use esp_radio::wifi::scan::{ScanConfig, ScanTypeConfig};

esp_bootloader_esp_idf::esp_app_desc!();

/// Nombre de scans a la suite (charge prolongee sur le rail 3V3).
const SCANS: u32 = 10;
/// Reseaux affiches (les plus forts).
const SHOWN: usize = 12;

fn auth_label(auth: Option<AuthenticationMethod>) -> &'static str {
    match auth {
        Some(AuthenticationMethod::None) => "ouvert",
        Some(AuthenticationMethod::Wep) => "WEP",
        Some(AuthenticationMethod::Wpa) => "WPA",
        Some(AuthenticationMethod::Wpa2Personal) | Some(AuthenticationMethod::WpaWpa2Personal) => "WPA2",
        Some(AuthenticationMethod::Wpa3Personal) | Some(AuthenticationMethod::Wpa2Wpa3Personal) => "WPA3",
        Some(_) => "autre",
        None => "?",
    }
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let _console = board_console!(peripherals); // avant tout message

    // Brownout surveille pendant le test, et raison du dernier reset.
    system::init();
    esp_println::println!(
        "== Scan WiFi == (dernier reset : {}{})",
        system::last_reset_label(),
        if system::last_reset_abnormal() { "  <-- ANORMAL" } else { "" }
    );

    // Tas memoire pour la pile radio (cf. documentation d'esp-radio).
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 36 * 1024);

    // Ordonnanceur de taches : obligatoire AVANT d'initialiser la radio.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);

    let (mut controller, _interfaces) = match esp_radio::wifi::new(peripherals.WIFI, Default::default()) {
        Ok(wifi) => wifi,
        Err(error) => {
            esp_println::println!("ECHEC : initialisation WiFi : {:?}", error);
            loop {}
        }
    };
    esp_println::println!("Radio WiFi demarree. {} scans actifs...", SCANS);

    // Ecoute de 50 a 120 ms par canal (comme ESP-IDF). Le defaut d'esp-radio
    // (10-20 ms) est trop court : la plupart des box n'ont pas le temps de
    // repondre (constate : 0 a 2 reseaux en 150 ms).
    let config = ScanConfig::default().with_scan_type(ScanTypeConfig::Active {
        min: Duration::from_millis(50),
        max: Duration::from_millis(120),
    });
    let mut networks: alloc::vec::Vec<AccessPointInfo> = alloc::vec::Vec::new();
    for scan in 1..=SCANS {
        let start = Instant::now();
        match embassy_futures::block_on(controller.scan_async(&config)) {
            Ok(found) => {
                esp_println::println!(
                    "scan {:2}/{} : {} reseau(x) en {} ms",
                    scan,
                    SCANS,
                    found.len(),
                    (Instant::now() - start).as_millis()
                );
                // Cumul sur tous les scans : un reseau par BSSID, meilleur signal garde.
                for ap in found {
                    match networks.iter_mut().find(|known: &&mut AccessPointInfo| known.bssid == ap.bssid) {
                        Some(known) if ap.signal_strength > known.signal_strength => *known = ap,
                        Some(_) => {}
                        None => networks.push(ap),
                    }
                }
            }
            Err(error) => esp_println::println!("scan {:2}/{} : erreur {:?}", scan, SCANS, error),
        }
    }

    networks.sort_by(|a, b| b.signal_strength.cmp(&a.signal_strength));
    esp_println::println!();
    esp_println::println!(
        "Reseaux vus sur les {} scans ({}), meilleur signal, du plus fort au plus faible :",
        SCANS,
        networks.len()
    );
    esp_println::println!("  dBm  canal  securite  nom");
    for ap in networks.iter().take(SHOWN) {
        let name = if ap.ssid.as_str().is_empty() { "(cache)" } else { ap.ssid.as_str() };
        esp_println::println!(
            "  {:4}  {:5}  {:8}  {}",
            ap.signal_strength,
            ap.channel,
            auth_label(ap.auth_method),
            name
        );
    }
    if networks.len() > SHOWN {
        esp_println::println!("  ... et {} autre(s)", networks.len() - SHOWN);
    }

    let (bod_on, bod_reset, _) = system::brownout_status();
    esp_println::println!();
    esp_println::println!(
        "== Fin : aucun redemarrage pendant {} scans (brownout actif={} reset={}) ==",
        SCANS,
        bod_on,
        bod_reset
    );
    loop {}
}
