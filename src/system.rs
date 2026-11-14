//! Etat du systeme : raison du dernier redemarrage, detecteur de brownout,
//! trace des panics.
//!
//! Brownout = chute de tension d'alimentation (ex. pic de courant du module LTE
//! en emission, batterie faible). Sous le seuil, l'ESP32 peut se comporter de
//! facon imprevisible ; le detecteur force alors un redemarrage propre et la
//! raison `SysBrownOut` est lisible au demarrage suivant.
//!
//! ATTENTION : sur l'ESP32, le detecteur est DESACTIVE a la mise sous tension
//! (registre RTC_CNTL_BROWN_OUT = 0x13FF0000 : ni ENA ni RST_ENA) et esp-hal ne
//! l'active pas (contrairement a ESP-IDF). Il faut appeler
//! `init()` (qui appelle `enable_brownout_reset()`) tot dans `main`.
//!
//! Si la tension descend encore plus bas (sous le seuil du power-on reset), la
//! puce redemarre comme a la mise sous tension : la raison lue est alors
//! `ChipPowerOn`, pas `SysBrownOut`.

use core::sync::atomic::{AtomicBool, Ordering};

use esp_hal::peripherals::LPWR;
use esp_hal::rtc_cntl::SocResetReason;

// ---------------------------------------------------------------------------
// Trace des panics
// Apres une panic, le gestionnaire (board.rs) redemarre la puce par un reset
// logiciel. Vu du materiel, c'est un reset logiciel "normal" : pour savoir
// qu'il y a eu une panic, on ecrit un marqueur dans la RAM RTC, qui n'est pas
// effacee par un reset logiciel ni par un watchdog (mais l'est a la mise sous
// tension, d'ou la valeur "magique" plutot qu'un simple booleen).
// ---------------------------------------------------------------------------

const PANIC_MAGIC: u32 = 0xDEAD_C0DE;

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut PANIC_MARKER: u32 = 0;

/// Vrai si le dernier redemarrage vient d'une panic (fixe par `init()`).
static LAST_WAS_PANIC: AtomicBool = AtomicBool::new(false);

/// A appeler une fois, tot dans `main` : lit puis efface le marqueur de panic
/// et active le detecteur de brownout.
pub fn init() {
    // SAFETY : un seul coeur utilise, appele avant toute panic possible ici.
    let marker = unsafe { core::ptr::addr_of_mut!(PANIC_MARKER).replace(0) };
    LAST_WAS_PANIC.store(marker == PANIC_MAGIC, Ordering::Relaxed);
    enable_brownout_reset();
}

/// Appele par le gestionnaire de panic juste avant le redemarrage.
pub fn record_panic() {
    // SAFETY : ecriture d'un u32 aligne ; le programme s'arrete juste apres.
    unsafe { core::ptr::addr_of_mut!(PANIC_MARKER).write(PANIC_MAGIC) };
}

/// Seuil du detecteur, de 0 (~2,43 V) a 7 (~2,80 V). 0 = defaut d'ESP-IDF.
/// Le rail 3V3_SYS (TPS63025) est a 3,3 V : 0 laisse de la marge sans
/// declencher sur du bruit normal.
pub const BROWNOUT_LEVEL: u8 = 0;

/// Active le detecteur de brownout avec redemarrage (comme ESP-IDF :
/// coupure RF et flash pendant la chute, attente max avant reset).
pub fn enable_brownout_reset() {
    LPWR::regs().brown_out().modify(|_, w| {
        // SAFETY : valeurs dans les plages des champs (3 bits et 10 bits).
        unsafe {
            w.dbrown_out_thres().bits(BROWNOUT_LEVEL);
            w.rst_wait().bits(0x3FF);
        }
        w.pd_rf_ena().set_bit();
        w.close_flash_ena().set_bit();
        w.rst_ena().set_bit();
        w.ena().set_bit()
    });
}

/// Etat du detecteur relu dans le registre : (actif, reset actif, seuil).
/// Sert a verifier sur la carte que `enable_brownout_reset()` a pris effet.
pub fn brownout_status() -> (bool, bool, u8) {
    let r = LPWR::regs().brown_out().read();
    (r.ena().bit(), r.rst_ena().bit(), r.dbrown_out_thres().bits())
}

/// Raison materielle du dernier redemarrage, `None` si illisible.
pub fn reset_reason() -> Option<SocResetReason> {
    esp_hal::system::reset_reason()
}

/// Texte court (majuscules, pour les palettes) decrivant le dernier
/// redemarrage. "PANIC" si le reset logiciel venait du gestionnaire de panic.
pub fn last_reset_label() -> &'static str {
    use SocResetReason::*;
    if LAST_WAS_PANIC.load(Ordering::Relaxed) {
        return "PANIC";
    }
    match reset_reason() {
        Some(ChipPowerOn) => "MISE SOUS TENSION",
        Some(CoreSw) | Some(Cpu0Sw) => "RESET LOGICIEL",
        Some(CoreDeepSleep) => "REVEIL DEEP SLEEP",
        Some(SysBrownOut) => "BROWNOUT",
        Some(CoreMwdt0) | Some(CoreMwdt1) | Some(CpuMwdt0) => "WATCHDOG",
        Some(CoreRtcWdt) | Some(Cpu0RtcWdt) | Some(SysRtcWdt) => "WATCHDOG RTC",
        Some(CoreSdio) => "RESET SDIO",
        Some(Cpu1Cpu0) => "RESET CPU1",
        None => "INCONNUE",
    }
}

/// Vrai si le dernier redemarrage n'est pas normal (panic, brownout,
/// watchdog...) : a signaler a l'utilisateur. Normaux : mise sous tension,
/// reset logiciel volontaire, reveil de deep sleep.
pub fn last_reset_abnormal() -> bool {
    use SocResetReason::*;
    LAST_WAS_PANIC.load(Ordering::Relaxed)
        || !matches!(reset_reason(), Some(ChipPowerOn) | Some(CoreSw) | Some(Cpu0Sw) | Some(CoreDeepSleep))
}
