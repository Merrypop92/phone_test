#![no_std]
#![no_main]

mod app;
mod calculator_app;
mod menu_app;
mod message_app;
mod task_app;

use app::{ActiveApp, AppCommand};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig};
use esp_hal::main;
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::uart::{Config as UartConfig, Uart};

use esp32_quick::{Epd, PX_HEIGHT, PX_WIDTH};

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    esp_println::println!("PANIC: {}", info);
    loop {}
}

esp_bootloader_esp_idf::esp_app_desc!();

const BUF_SIZE: usize = (PX_WIDTH as usize * PX_HEIGHT as usize) / 8;
static mut FRAME_BUFFER: [u8; BUF_SIZE] = [0xFF; BUF_SIZE];

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut delay = Delay::new();

    let mut serial = Uart::new(peripherals.UART0, UartConfig::default())
        .expect("echec init UART0")
        .with_rx(peripherals.GPIO3)
        .with_tx(peripherals.GPIO1);

    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(4))
            .with_mode(Mode::_0),
    )
    .expect("echec init SPI")
    .with_sck(peripherals.GPIO18)
    .with_mosi(peripherals.GPIO23);

    let cs = Output::new(peripherals.GPIO13, Level::High, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let rst = Output::new(peripherals.GPIO12, Level::High, OutputConfig::default());
    let busy = Input::new(peripherals.GPIO5, InputConfig::default());
    let mut epd = Epd::new(spi, cs, dc, rst, busy);

    // SAFETY: acces exclusif au buffer, reutilise uniquement dans cette boucle.
    let buf: &mut [u8] = unsafe { &mut *core::ptr::addr_of_mut!(FRAME_BUFFER) };

    epd.hw_init(&mut delay);

    let mut current_app = ActiveApp::new();
    current_app.draw(&mut epd, buf);
    esp_println::println!("Menu pret : UP, DOWN, ENTER, BACK puis Entree.");

    let mut line = [0u8; 96];
    let mut length = 0usize;
    let mut byte = [0u8; 1];

    loop {
        if !serial.read_ready() {
            continue;
        }

        match serial.read(&mut byte) {
            Ok(1) if byte[0] == b'\r' || byte[0] == b'\n' => {
                if length > 0 {
                    let input = core::str::from_utf8(&line[..length])
                        .unwrap_or("Texte non UTF-8");

                    let action = current_app.handle_input(input, &mut epd, buf);
                    match action {
                        AppCommand::OpenMessages => {
                            esp_println::println!("Messages : texte libre ou BACK.");
                        }
                        AppCommand::OpenTasks => {
                            esp_println::println!("Taches : ADD <texte>, EDIT <n> <texte>, DONE <n>, UNDO <n>, DELETE <n>, LIST ou BACK.");
                        }
                        AppCommand::OpenCalculator => {
                            esp_println::println!("Calculatrice : expression (ex. 12 + 3), CLEAR ou BACK.");
                        }
                        AppCommand::Stay | AppCommand::BackToMenu => {}
                    }
                    current_app.activate(action, &mut epd, buf);
                    length = 0;
                }
            }
            Ok(1) if length < line.len() => {
                line[length] = byte[0];
                length += 1;
            }
            Ok(1) => {
                esp_println::println!("Ligne trop longue");
                length = 0;
            }
            Ok(_) | Err(_) => {}
        }
    }
}