//! Carte DocPhone : cablage (quelles broches pour quoi) et initialisation du
//! materiel, regroupes a un seul endroit.
//!
//! Chaque sous-systeme a une macro qui preleve les bons peripheriques dans la
//! structure renvoyee par `esp_hal::init()` et appelle la fonction d'init :
//!
//! ```ignore
//! let p = esp_hal::init(esp_hal::Config::default());
//! let mut epd = esp32_quick::board_display!(p);
//! let mut console = esp32_quick::board_console!(p);
//! ```
//!
//! Les broches ne sont donc ecrites qu'une fois (dans les macros ci-dessous) ;
//! les fonctions d'init prennent des types de broches precis, donc une erreur
//! de cablage dans une macro est detectee a la compilation.
//!
//! Pour changer une broche : modifier la macro ET le type correspondant dans
//! la fonction d'init.

use core::sync::atomic::{AtomicBool, Ordering};

use esp_hal::Blocking;
use esp_hal::analog::adc::{Adc, AdcConfig, AdcPin, Attenuation};
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals::{
    ADC1, GPIO1, GPIO2, GPIO3, GPIO5, GPIO12, GPIO13, GPIO14, GPIO18, GPIO21, GPIO22, GPIO23, GPIO25,
    GPIO26, GPIO27, GPIO32, GPIO39, I2C0, SPI2, TIMG0, UART0, UART1,
};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::{Duration, Rate};
use esp_hal::timer::timg::{MwdtStage, TimerGroup, Wdt};
use esp_hal::uart::{Config as UartConfig, Uart};

use crate::{BUF_SIZE, Epd};

// ---------------------------------------------------------------------------
// Ecran e-paper GDEQ0426T82 (SSD1677)
// SPI2 a 10 MHz (SSD1677 : jusqu'a 20 MHz en ecriture) : SCK=GPIO18,
// MOSI=GPIO23 (bus partage avec la carte SD)
// CS=GPIO13, DC=GPIO2, RST=GPIO12, BUSY=GPIO5 (confirmes par le driver C de reference)
// ---------------------------------------------------------------------------

/// Initialise l'ecran e-paper. Utiliser la macro `board_display!(peripherals)`.
pub fn display(
    spi: SPI2<'static>,
    sck: GPIO18<'static>,
    mosi: GPIO23<'static>,
    cs: GPIO13<'static>,
    dc: GPIO2<'static>,
    rst: GPIO12<'static>,
    busy: GPIO5<'static>,
) -> Epd<'static> {
    let spi = Spi::new(
        spi,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(10))
            .with_mode(Mode::_0),
    )
    .expect("echec init SPI")
    .with_sck(sck)
    .with_mosi(mosi);

    Epd::new(
        spi,
        Output::new(cs, Level::High, OutputConfig::default()),
        Output::new(dc, Level::Low, OutputConfig::default()),
        Output::new(rst, Level::High, OutputConfig::default()),
        Input::new(busy, InputConfig::default()),
    )
}

#[macro_export]
macro_rules! board_display {
    ($p:ident) => {
        $crate::board::display($p.SPI2, $p.GPIO18, $p.GPIO23, $p.GPIO13, $p.GPIO2, $p.GPIO12, $p.GPIO5)
    };
}

// ---------------------------------------------------------------------------
// Console serie USB (UART0) : RX=GPIO3, TX=GPIO1
// ---------------------------------------------------------------------------

/// Initialise la console serie USB. Utiliser la macro `board_console!(peripherals)`.
pub fn console(uart: UART0<'static>, rx: GPIO3<'static>, tx: GPIO1<'static>) -> Uart<'static, Blocking> {
    Uart::new(uart, UartConfig::default())
        .expect("echec init UART0")
        .with_rx(rx)
        .with_tx(tx)
}

#[macro_export]
macro_rules! board_console {
    ($p:ident) => {
        $crate::board::console($p.UART0, $p.GPIO3, $p.GPIO1)
    };
}

// ---------------------------------------------------------------------------
// Matrice de boutons TCA8418 (U11, I2C0, 100 kHz) : SDA=GPIO21, SCL=GPIO22, INT#=GPIO25
// Pull-ups SDA/SCL : 4,7 kOhm (R_7, R_8). RESET# : pull-up 10 kOhm vers 3V3 (R_6).
// INT# : sur le schema, R_9 (10 kOhm) est EN SERIE entre INT# et GPIO25, sans
// pull-up ; on active donc le pull-up interne de l'ESP32 (a corriger sur la
// prochaine version du PCB : R_9 entre INT# et 3V3).
// ATTENTION : sur le schema, VCC (broche 21) n'est relie qu'a C_13 (100 nF), pas
// au 3V3 (confirme) : la puce n'est pas alimentee tant que la liaison VCC -> 3V3
// n'est pas ajoutee. Verifier ensuite avec examples/i2c_scan.rs.
// ---------------------------------------------------------------------------

/// Adresse I2C du TCA8418.
pub const KEYPAD_I2C_ADDR: u8 = 0x34;

/// Initialise le bus I2C de la matrice de boutons et sa broche d'interruption.
/// Utiliser la macro `board_keypad!(peripherals)`.
pub fn keypad(
    i2c: I2C0<'static>,
    sda: GPIO21<'static>,
    scl: GPIO22<'static>,
    int: GPIO25<'static>,
) -> (I2c<'static, Blocking>, Input<'static>) {
    let i2c = I2c::new(i2c, I2cConfig::default().with_frequency(Rate::from_khz(100)))
        .expect("echec init I2C")
        .with_sda(sda)
        .with_scl(scl);
    (i2c, Input::new(int, InputConfig::default().with_pull(Pull::Up)))
}

#[macro_export]
macro_rules! board_keypad {
    ($p:ident) => {
        $crate::board::keypad($p.I2C0, $p.GPIO21, $p.GPIO22, $p.GPIO25)
    };
}

// ---------------------------------------------------------------------------
// Module LTE A7682E (cf. PaxOS-9, a confirmer selon le schema de cette carte)
// UART1 : RX=GPIO26 (ESP <- module), TX=GPIO27 (ESP -> module) ; PWRKEY=GPIO32
// ---------------------------------------------------------------------------

/// Initialise l'UART du module LTE et sa broche PWRKEY (au repos a LOW).
/// Utiliser la macro `board_lte!(peripherals)`.
pub fn lte(
    uart: UART1<'static>,
    rx: GPIO26<'static>,
    tx: GPIO27<'static>,
    pwrkey: GPIO32<'static>,
) -> (Uart<'static, Blocking>, Output<'static>) {
    let uart = Uart::new(uart, UartConfig::default())
        .expect("echec init UART LTE")
        .with_rx(rx)
        .with_tx(tx);
    (uart, Output::new(pwrkey, Level::Low, OutputConfig::default()))
}

#[macro_export]
macro_rules! board_lte {
    ($p:ident) => {
        $crate::board::lte($p.UART1, $p.GPIO26, $p.GPIO27, $p.GPIO32)
    };
}

// ---------------------------------------------------------------------------
// Etat de charge : sortie STATUS/CHRG du TP4056 sur GPIO39 (ADC1, 11 dB)
// cf. PaxOS-9 (PIN_BATTERY_CHARGING_STATUS). Valide : lecture brute ~2950 en charge.
// ---------------------------------------------------------------------------

pub type ChargeStatusPin = AdcPin<GPIO39<'static>, ADC1<'static>>;

/// Initialise l'ADC de detection de charge. Lire avec
/// `nb::block!(adc.read_oneshot(&mut pin))`. Utiliser `board_charge_status!(peripherals)`.
pub fn charge_status(adc: ADC1<'static>, pin: GPIO39<'static>) -> (Adc<'static, ADC1<'static>, Blocking>, ChargeStatusPin) {
    let mut config = AdcConfig::new();
    let pin = config.enable_pin(pin, Attenuation::_11dB);
    (Adc::new(adc, config), pin)
}

#[macro_export]
macro_rules! board_charge_status {
    ($p:ident) => {
        $crate::board::charge_status($p.ADC1, $p.GPIO39)
    };
}

// ---------------------------------------------------------------------------
// LED : GPIO14 (suppose active a l'etat haut ; a confirmer sur le schema).
// Attention : GPIO14 est aussi MTMS (JTAG) et sort un signal pendant le boot.
// ---------------------------------------------------------------------------

/// Initialise la LED, eteinte. Utiliser la macro `board_led!(peripherals)`.
pub fn led(pin: GPIO14<'static>) -> Output<'static> {
    Output::new(pin, Level::Low, OutputConfig::default())
}

#[macro_export]
macro_rules! board_led {
    ($p:ident) => {
        $crate::board::led($p.GPIO14)
    };
}

/// Fait clignoter la LED `count` fois (on/off de `ms` millisecondes), puis
/// la laisse eteinte (economie de batterie).
pub fn blink(led: &mut Output<'_>, count: u32, ms: u32) {
    for _ in 0..count {
        led.set_high();
        delay_ms(ms);
        led.set_low();
        delay_ms(ms);
    }
}

// ---------------------------------------------------------------------------
// Autres broches du schema, pas encore utilisees par le logiciel :
//   VIBRATOR=GPIO33, SW_HOME=GPIO15 (bouton home),
//   SD_CS=GPIO4 et SPI_MISO=GPIO19 (carte SD, sur le meme bus SPI que l'ecran).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Watchdog : celui du groupe de timers 0 (MWDT0). S'il n'est pas "nourri"
// (`feed()`) pendant WATCHDOG_TIMEOUT_S, il redemarre la puce ; la raison
// lue au demarrage suivant est alors WATCHDOG.
// Le delai doit depasser la plus longue attente normale : un rafraichissement
// complet de l'e-paper bloque ~3,5 s (plus par temps froid).
// ---------------------------------------------------------------------------

pub const WATCHDOG_TIMEOUT_S: u64 = 10;

/// Demarre le watchdog. Appeler `feed()` regulierement dans la boucle
/// principale. Utiliser la macro `board_watchdog!(peripherals)`.
pub fn watchdog(timg: TIMG0<'static>) -> Wdt<TIMG0<'static>> {
    let mut wdt = TimerGroup::new(timg).wdt;
    wdt.set_timeout(MwdtStage::Stage0, Duration::from_secs(WATCHDOG_TIMEOUT_S));
    wdt.enable();
    wdt
}

#[macro_export]
macro_rules! board_watchdog {
    ($p:ident) => {
        $crate::board::watchdog($p.TIMG0)
    };
}

// ---------------------------------------------------------------------------
// Tampon d'image plein ecran (48 Ko, 1 bit par pixel)
// ---------------------------------------------------------------------------

static mut FRAME_BUFFER: [u8; BUF_SIZE] = [0xFF; BUF_SIZE];
static FRAME_BUFFER_TAKEN: AtomicBool = AtomicBool::new(false);

/// Donne acces au tampon d'image plein ecran. Ne peut etre appele qu'une fois
/// (panique sinon) : c'est ce qui garantit qu'il n'y a qu'un seul acces.
pub fn frame_buffer() -> &'static mut [u8] {
    if FRAME_BUFFER_TAKEN.swap(true, Ordering::AcqRel) {
        panic!("board::frame_buffer() appele deux fois");
    }
    // SAFETY : le drapeau ci-dessus garantit qu'une seule reference existe.
    unsafe { &mut *core::ptr::addr_of_mut!(FRAME_BUFFER) }
}

// ---------------------------------------------------------------------------
// Divers
// ---------------------------------------------------------------------------

/// Attente active en millisecondes.
pub fn delay_ms(ms: u32) {
    esp_hal::delay::Delay::new().delay_millis(ms);
}

/// En cas de panic : message sur la console, marqueur en RAM RTC (pour
/// afficher "PANIC" au demarrage suivant), puis redemarrage. La pause de 2 s
/// laisse le temps de lire le message et evite une boucle de redemarrages trop
/// rapide si la panic se reproduit a chaque demarrage.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    esp_println::println!("PANIC: {}", info);
    esp_println::println!("Redemarrage dans 2 s...");
    crate::system::record_panic();
    delay_ms(2000);
    esp_hal::system::software_reset()
}
