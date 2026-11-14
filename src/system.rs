//! Etat du systeme : raison du dernier redemarrage et detecteur de brownout.
//!
//! Brownout = chute de tension d'alimentation (ex. pic de courant du module LTE
//! en emission, batterie faible). Sous le seuil, l'ESP32 peut se comporter de
//! facon imprevisible ; le detecteur force alors un redemarrage propre et la
//! raison `SysBrownOut` est lisible au demarrage suivant.
//!
//! ATTENTION : sur l'ESP32, le detecteur est DESACTIVE a la mise sous tension
//! (registre RTC_CNTL_BROWN_OUT = 0x13FF0000 : ni ENA ni RST_ENA) et esp-hal ne
//! l'active pas (contrairement a ESP-IDF). Il faut appeler
//! `enable_brownout_reset()` tot dans `main`.
//!
//! Si la tension descend encore plus bas (sous le seuil du power-on reset), la
//! puce redemarre comme a la mise sous tension : la raison lue est alors
//! `ChipPowerOn`, pas `SysBrownOut`.

use esp_hal::peripherals::LPWR;
use esp_hal::rtc_cntl::SocResetReason;

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

/// Raison du dernier redemarrage, `None` si illisible.
pub fn reset_reason() -> Option<SocResetReason> {
    esp_hal::system::reset_reason()
}

/// Texte court (majuscules, pour les palettes) decrivant une raison de reset.
pub fn reset_reason_label(reason: Option<SocResetReason>) -> &'static str {
    use SocResetReason::*;
    match reason {
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

/// Vrai si le redemarrage n'est pas normal (ni mise sous tension, ni reset
/// logiciel/flash, ni reveil de deep sleep) : a signaler a l'utilisateur.
pub fn reset_is_abnormal(reason: Option<SocResetReason>) -> bool {
    use SocResetReason::*;
    !matches!(reason, Some(ChipPowerOn) | Some(CoreSw) | Some(Cpu0Sw) | Some(CoreDeepSleep))
}
