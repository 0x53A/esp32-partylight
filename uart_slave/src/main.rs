#![no_std]
#![no_main]
#![feature(never_type)]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    dma_buffers,
    gpio::Io,
    i2s::master::{DataFormat, Standard},
    main,
    peripherals::Peripherals,
    time::Rate,
    uart::Uart,
};

use anyhow::{Result, anyhow};
use esp_println::println;
use heapless::Vec;

use esp_alloc as _;
use microfft::real::rfft_512;
use smart_leds::RGB8;
use smart_leds::SmartLedsWrite;

type NeopixelT<'a> = ws2812_spi::Ws2812<esp_hal::spi::master::Spi<'a, esp_hal::Blocking>>;

#[main]
fn main() -> ! {
    println!("Hello, world!");

    match _main() {
        Err(_e) => loop {},
    }
}

fn update_led(r: u8, g: u8, b: u8, neopixel: &mut NeopixelT) -> Result<()> {
    let color = RGB8::new(r, g, b);

    // Update neopixel
    neopixel
        .write([color])
        .map_err(|err| anyhow!("{:?}", err))?;

    Ok(())
}

fn hex_to_u8(hex_str: &str) -> Result<u8> {
    if hex_str.len() != 2 {
        return Err(anyhow!("Invalid hex length"));
    }

    let mut result = 0u8;
    for c in hex_str.chars() {
        result = result << 4;
        match c {
            '0'..='9' => result += (c as u8) - b'0',
            'a'..='f' => result += (c as u8) - b'a' + 10,
            'A'..='F' => result += (c as u8) - b'A' + 10,
            _ => return Err(anyhow!("Invalid hex character")),
        }
    }
    Ok(result)
}

fn parse_rgb_command(line: &str) -> Result<(u8, u8, u8)> {
    // Expected format: "r=11;g=aa;b=ff;"
    let mut r = 0u8;
    let mut g = 0u8;
    let mut b = 0u8;

    // Split by ';' and parse each part
    for part in line.split(';') {
        if part.is_empty() {
            continue;
        }

        if let Some(eq_pos) = part.find('=') {
            let key = &part[..eq_pos];
            let value = &part[eq_pos + 1..];

            match key {
                "r" => r = hex_to_u8(value)?,
                "g" => g = hex_to_u8(value)?,
                "b" => b = hex_to_u8(value)?,
                _ => return Err(anyhow!("Unknown key: {}", key)),
            }
        }
    }

    Ok((r, g, b))
}

fn _main() -> Result<!> {
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let peripherals: Peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();
    let io = Io::new(peripherals.IO_MUX);

    // Setup SPI for NeoPixel
    let spi = esp_hal::spi::master::Spi::new(
        peripherals.SPI2,
        esp_hal::spi::master::Config::default().with_frequency(Rate::from_mhz(4)),
    )?
    .with_mosi(peripherals.GPIO48);

    let mut neopixel: NeopixelT = ws2812_spi::Ws2812::new(spi);

    // Setup UART
    let config = esp_hal::uart::Config::default().with_baudrate(115200);
    let mut uart = Uart::new(peripherals.UART1, config)?
        .with_rx(peripherals.GPIO17)
        .with_tx(peripherals.GPIO8);

    let blue = smart_leds::colors::BLUE;
    neopixel.write([blue]).map_err(|err| anyhow!("{:?}", err))?;

    let mut buffer: Vec<u8, 256> = Vec::new();

    loop {
        // Read bytes from UART
        let mut read_buffer = [0u8; 32];
        if let Ok(n_read) = uart.read(&mut read_buffer) {
            for received in read_buffer[0..n_read].iter() {
                if *received == b'\n' {
                    // Process complete line
                    if let Ok(line) = core::str::from_utf8(&buffer) {
                        match parse_rgb_command(line) {
                            Ok((r, g, b)) => {
                                // println!("Setting RGB: r={}, g={}, b={}", r, g, b);
                                if let Err(e) = update_led(r, g, b, &mut neopixel) {
                                    println!("LED update error: {:?}", e);
                                }
                            }
                            Err(e) => {
                                println!("Parse error: {:?}", e);
                            }
                        }
                    }
                    buffer.clear();
                } else if *received != b'\r' {
                    // Add to buffer (ignore carriage return)
                    if buffer.push(*received).is_err() {
                        // Buffer full, clear it
                        buffer.clear();
                        println!("Buffer overflow, cleared");
                    }
                }
            }
        }
    }
}
