#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use esp_hal::analog::adc::{Adc, AdcConfig, Attenuation};
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::main;
use esp_hal::time::{Duration, Instant};
use esp_hal::uart::{Config as UartConfig, Uart};

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    esp_println::println!("PANIC: {}", info);
    loop {}
}

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

fn delay_ms(ms: u64) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(ms) {}
}

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    // generator version: 1.3.0
    // generator parameters: --chip esp32

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // TP4056 STATUS/CHRG pin, cf. PaxOS-9 (PIN_BATTERY_CHARGING_STATUS = GPIO39 sur la
    // PAXO v5). Validé sur cette carte : raw ~2950 en charge.
    let mut adc_config = AdcConfig::new();
    let mut charge_status_pin = adc_config.enable_pin(peripherals.GPIO39, Attenuation::_11dB);
    let mut adc1 = Adc::new(peripherals.ADC1, adc_config);

    // Module LTE (A7682E), cf. PaxOS-9 : PWRKEY=GPIO32, RX=GPIO26 (ESP <- module),
    // TX=GPIO27 (ESP -> module). A confirmer/ajuster selon le schéma de cette carte.
    let mut pwrkey = Output::new(peripherals.GPIO32, Level::Low, OutputConfig::default());
    let mut lte_uart = Uart::new(peripherals.UART1, UartConfig::default())
        .expect("échec init UART LTE")
        .with_rx(peripherals.GPIO26)
        .with_tx(peripherals.GPIO27);

    // --- Phase 1 : test PWRKEY mesurable au multimètre ---
    // Bascule GPIO32 lentement pour pouvoir sonder la grille et le drain de Q5.
    esp_println::println!("== TEST PWRKEY : GPIO32 bascule 1s HIGH / 1s LOW pendant 12s ==");
    esp_println::println!("   -> sonde maintenant la grille puis le drain de Q5 (2N7002).");
    for i in 1..=6 {
        pwrkey.set_level(Level::High);
        esp_println::println!("  cycle {i}: PWRKEY(GPIO32) = HIGH");
        delay_ms(1000);
        pwrkey.set_level(Level::Low);
        esp_println::println!("  cycle {i}: PWRKEY(GPIO32) = LOW");
        delay_ms(1000);
    }

    // --- Phase 2 : tentatives d'allumage + handshake AT, avec retry facon PaxOS-9 ---
    // 1024 pour laisser de la place a une reponse AT+COPS=? (liste d'operateurs, peut etre longue).
    let mut rx_buf = [0u8; 1024];
    let mut attempt = 0u32;

    loop {
        attempt += 1;
        esp_println::println!("[tentative {attempt}] pulse PWRKEY (HIGH 800ms), puis boot 8s...");
        pwrkey.set_level(Level::High);
        delay_ms(800);
        pwrkey.set_level(Level::Low);
        delay_ms(8000);

        let mut got_ok = false;
        for _ in 0..10 {
            let raw: u16 = nb::block!(adc1.read_oneshot(&mut charge_status_pin)).unwrap_or(0);
            esp_println::println!("charge raw={raw} charging={}", raw > 100);

            let _ = lte_uart.write(b"AT\r\n");
            let _ = lte_uart.flush();
            delay_ms(300);

            let mut total = 0usize;
            while lte_uart.read_ready() && total < rx_buf.len() {
                match lte_uart.read(&mut rx_buf[total..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => total += n,
                }
            }

            if total > 0 {
                match core::str::from_utf8(&rx_buf[..total]) {
                    Ok(s) => esp_println::println!("LTE RX: {s:?}"),
                    Err(_) => {
                        esp_println::println!("LTE RX ({total} o non-UTF8): {:?}", &rx_buf[..total])
                    }
                }
                if rx_buf[..total].windows(2).any(|w| w == b"OK") {
                    got_ok = true;
                }
            } else {
                esp_println::println!("LTE RX: (rien)");
            }
            delay_ms(400);
        }

        if got_ok {
            esp_println::println!("== MODULE LTE OK, deverrouillage SIM (une seule tentative) ==");
            let _ = lte_uart.write(b"AT+CPIN=\"0000\"\r\n");
            let _ = lte_uart.flush();
            delay_ms(1000);
            let mut total = 0usize;
            while lte_uart.read_ready() && total < rx_buf.len() {
                match lte_uart.read(&mut rx_buf[total..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => total += n,
                }
            }
            if total > 0 {
                if let Ok(s) = core::str::from_utf8(&rx_buf[..total]) {
                    esp_println::println!("LTE RX (deverrouillage): {s:?}");
                }
            } else {
                esp_println::println!("LTE RX (deverrouillage): (rien)");
            }
            delay_ms(2000); // laisse le temps au module de traiter et de s'attacher

            // Scan manuel des operateurs visibles (AT+COPS=?) : diagnostic ponctuel,
            // peut prendre jusqu'a 60s (scan RF complet), donc hors de la boucle rapide.
            esp_println::println!("Scan operateurs (AT+COPS=?), jusqu'a 60s...");
            let _ = lte_uart.write(b"AT+COPS=?\r\n");
            let _ = lte_uart.flush();
            delay_ms(60000);
            let mut total = 0usize;
            while lte_uart.read_ready() && total < rx_buf.len() {
                match lte_uart.read(&mut rx_buf[total..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => total += n,
                }
            }
            if total > 0 {
                match core::str::from_utf8(&rx_buf[..total]) {
                    Ok(s) => esp_println::println!("LTE RX (scan operateurs): {s:?}"),
                    Err(_) => esp_println::println!(
                        "LTE RX (scan operateurs, {total} o non-UTF8): {:?}",
                        &rx_buf[..total]
                    ),
                }
            } else {
                esp_println::println!("LTE RX (scan operateurs): (rien)");
            }

            esp_println::println!("== diagnostic SIM/reseau en boucle ==");
            // AT+CPIN?  -> etat de la SIM (READY, SIM PIN, etc.)
            // AT+CREG?  -> enregistrement reseau 2G/3G (GSM/UMTS)
            // AT+CEREG? -> enregistrement reseau LTE (EPS), a surveiller separement de CREG
            // AT+COPS?  -> operateur actuellement selectionne (si enregistre)
            // AT+CSQ    -> qualite du signal
            let commands: [&[u8]; 5] = [
                b"AT+CPIN?\r\n",
                b"AT+CREG?\r\n",
                b"AT+CEREG?\r\n",
                b"AT+COPS?\r\n",
                b"AT+CSQ\r\n",
            ];
            loop {
                for cmd in commands {
                    let _ = lte_uart.write(cmd);
                    let _ = lte_uart.flush();
                    delay_ms(500);
                    let mut total = 0usize;
                    while lte_uart.read_ready() && total < rx_buf.len() {
                        match lte_uart.read(&mut rx_buf[total..]) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => total += n,
                        }
                    }
                    if total > 0 {
                        if let Ok(s) = core::str::from_utf8(&rx_buf[..total]) {
                            esp_println::println!("LTE RX: {s:?}");
                        }
                    }
                    delay_ms(800);
                }
            }
        }

        esp_println::println!("[tentative {attempt}] pas de reponse.");
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples
}
