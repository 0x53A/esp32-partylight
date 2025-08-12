#![no_std]
#![no_main]
#![feature(never_type)]

use core::fmt::Write as _;

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    dma_buffers,
    i2s::master::{DataFormat, Standard},
    main,
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

use esp_hal::peripherals::Peripherals;

type NeopixelT<'a> = ws2812_spi::Ws2812<esp_hal::spi::master::Spi<'a, esp_hal::Blocking>>;

#[main]
fn main() -> ! {
    println!("Hello, world!");

    match _main() {
        Err(_e) => loop {},
    }
}

fn process_audio_samples(
    buffer: &[u8],
) -> Result<(heapless::Vec<i32, 1024>, heapless::Vec<i32, 1024>)> {
    if buffer.len() % 8 != 0 {
        return Err(anyhow!("Buffer length must be a multiple of 8"));
    }

    let mut left_samples = heapless::Vec::new();
    let mut right_samples = heapless::Vec::new();

    for chunk in buffer.chunks_exact(8) {
        let left_value = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        left_samples.push(left_value);

        let right_value = i32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        right_samples.push(right_value);
    }

    Ok((left_samples, right_samples))
}

fn process_fft_and_update_led(
    samples: &[i32],
    neopixel: &mut NeopixelT,
    uart: &mut Uart<'_, esp_hal::Blocking>,
) -> Result<()> {
    // Take up to 512 samples, pad with zeros if needed
    let mut fft_input = [0.0f32; 512];
    let sample_count = core::cmp::min(samples.len(), 512);

    // Normalize and copy samples
    const MAX_VALUE: f32 = (1 << 23) as f32;
    for (i, &sample) in samples.iter().take(sample_count).enumerate() {
        fft_input[i] = (sample as f32) / MAX_VALUE;
    }

    // Perform FFT
    let spectrum = rfft_512(&mut fft_input);

    // Clear Nyquist frequency imaginary part
    spectrum[0].im = 0.0;

    // Group frequency bands (48kHz/512 = ~94Hz per bin)
    // Bass: bin 1 (~94Hz)
    // Mid: bins 2-10
    // Treble: bins 11-20 (~1034-1880Hz)

    let bass_energy: f32 = spectrum[1..2].iter().map(|c| c.norm_sqr()).sum();
    let mid_energy: f32 = spectrum[2..11].iter().map(|c| c.norm_sqr()).sum();
    let treble_energy: f32 = spectrum[11..21].iter().map(|c| c.norm_sqr()).sum();

    // Normalize energies (tweak these values)
    let bass = (bass_energy * 0.001).min(255.0) as u8;
    let mid = (mid_energy * 0.0001).min(255.0) as u8;
    let treble = (treble_energy * 0.001).min(255.0) as u8;

    let color = RGB8::new(bass, 0, 0);

    neopixel
        .write([color])
        .map_err(|err| anyhow!("{:?}", err))?;

    send_rgb_uart(0, mid, 0, uart)?;

    // println!(
    //     "Bass: {}, Mid: {}, Treble: {} -> Color: ({},{},{})",
    //     bass_energy, mid_energy, treble_energy, bass, mid, treble
    // );

    Ok(())
}

fn send_rgb_uart(r: u8, g: u8, b: u8, uart: &mut Uart<'_, esp_hal::Blocking>) -> Result<()> {
    let mut string: heapless::String<256> = heapless::String::new();
    writeln!(&mut string, "r={:02x};g={:02x};b={:02x};", r, g, b)?;

    uart.write(string.as_bytes())
        .map_err(|e| anyhow!("UART write error: {:?}", e))?;

    Ok(())
}

fn _main() -> Result<!> {
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let peripherals: Peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();

    // Neopixel setup
    let spi = esp_hal::spi::master::Spi::new(
        peripherals.SPI2,
        esp_hal::spi::master::Config::default().with_frequency(Rate::from_mhz(4)),
    )?
    .with_mosi(peripherals.GPIO48);

    let mut neopixel: NeopixelT = ws2812_spi::Ws2812::new(spi);

    let blue = smart_leds::colors::BLUE;
    neopixel.write([blue]).map_err(|err| anyhow!("{:?}", err))?;

    // UART setup
    let config = esp_hal::uart::Config::default().with_baudrate(115200);
    let mut uart: Uart<'_, esp_hal::Blocking> = Uart::new(peripherals.UART1, config)?
        .with_rx(peripherals.GPIO17)
        .with_tx(peripherals.GPIO8);

    // I2S setup
    let (mut rx_buffer, rx_descriptors, _, _) = dma_buffers!(4 * 4092, 0);
    let i2s = esp_hal::i2s::master::I2s::new(
        peripherals.I2S0,
        Standard::Philips,
        DataFormat::Data32Channel32,
        Rate::from_khz(48),
        peripherals.DMA_CH0,
    );

    let i2s = i2s.with_mclk(peripherals.GPIO0);
    let mut i2s_rx = i2s
        .i2s_rx
        .with_bclk(peripherals.GPIO4)
        .with_ws(peripherals.GPIO6)
        .with_din(peripherals.GPIO5)
        .build(rx_descriptors);

    let mut transfer = i2s_rx
        .read_dma_circular(&mut rx_buffer)
        .map_err(|err| anyhow!("{:?}", err))?;

    // println!("I2S initialized, starting audio processing...");

    loop {
        let avail = transfer.available().map_err(|err| anyhow!("{:?}", err))?;

        if avail > 0 {
            let mut rcv = [0u8; 5000];
            let bytes_read = core::cmp::min(avail, rcv.len());
            transfer
                .pop(&mut rcv[..bytes_read])
                .map_err(|err| anyhow!("{:?}", err))?;

            match process_audio_samples(&rcv[..bytes_read]) {
                Ok((left_samples, _right_samples)) => {
                    if left_samples.len() >= 128 {
                        if let Err(e) =
                            process_fft_and_update_led(&left_samples, &mut neopixel, &mut uart)
                        {
                            println!("FFT processing error: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("Audio processing error: {:?}", e);
                }
            }
        }
    }
}
