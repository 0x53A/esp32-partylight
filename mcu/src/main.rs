#![no_std]
#![no_main]
#![feature(never_type)]

extern crate alloc;
use alloc::{boxed::Box, format};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use esp_hal_embassy::Executor;

use core::ptr::addr_of_mut;

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    dma::{DmaRxBuf, DmaTxBuf},
    dma_buffers,
    i2s::master::{DataFormat, Standard},
    system::{CpuControl, Stack},
    time::Rate,
    timer::{AnyTimer, timg::TimerGroup},
};

use anyhow::{Result, anyhow};
use esp_println::println;

use esp_alloc as _;
use microfft::{Complex32, real::rfft_512};
use smart_leds::RGB8;
use smart_leds::SmartLedsWrite;

use esp_hal::peripherals::Peripherals;

use static_cell::StaticCell;

type NeopixelT<'a> = ws2812_spi::prerendered::Ws2812<
    'static,
    esp_hal::spi::master::SpiDmaBus<'a, esp_hal::Blocking>,
>;

static mut APP_CORE_STACK: Stack<8192> = Stack::new();

const MATRIX_LENGTH: usize = 16 * 16;
const LED_STRIP_LENGTH: usize = 0;
const TOTAL_NEOPIXEL_LENGTH: usize = MATRIX_LENGTH + LED_STRIP_LENGTH;

macro_rules! error_with_location {
    ($msg:expr) => {
        anyhow!("{} at {}:{}", $msg, file!(), line!())
    };
    ($fmt:expr, $($arg:tt)*) => {
        anyhow!("{} at {}:{}", format!($fmt, $($arg)*), file!(), line!())
    };
}

#[embassy_executor::task]
async fn neopixel_task(
    spi: esp_hal::spi::master::SpiDmaBus<'static, esp_hal::Blocking>,
    control: &'static Signal<CriticalSectionRawMutex, Box<[RGB8]>>,
) -> ! {
    println!("Neopixel task started");

    let neopixel_buffer = Box::leak(Box::new([0u8; 12 * TOTAL_NEOPIXEL_LENGTH + 140]));
    let mut neopixel: NeopixelT = ws2812_spi::prerendered::Ws2812::new(spi, neopixel_buffer);

    neopixel_demo(&mut neopixel);

    loop {
        let new_data = control.wait().await;
        let write_result = neopixel
            .write(new_data)
            .map_err(|err| error_with_location!("Failed to write to neopixel: {:?}", err));
        if let Err(e) = write_result {
            println!("{:?}", e);
        }
    }
}

fn neopixel_demo(neopixel: &mut NeopixelT) {
    let started = esp_hal::time::Instant::now();
    let mut i = 0;
    loop {
        // Demo: Three sine waves cycling through the 16x16 matrix
        // Red starts at 0, Blue at 1/3, Green at 2/3 of the cycle
        let mut colors = [RGB8::new(0, 0, 0); 256];

        let time_offset = (i as f32) * 0.1; // Animation speed

        for led_index in 0..256 {
            let position = (led_index as f32) / 256.0 * 2.0 * core::f32::consts::PI;

            // Three sine waves offset by 2π/3 (120 degrees)
            let red_phase = position + time_offset;
            let blue_phase = position + time_offset + 2.0 * core::f32::consts::PI / 3.0;
            let green_phase = position + time_offset + 4.0 * core::f32::consts::PI / 3.0;

            // Calculate sine values and convert to 0-255 range
            let red = ((libm::sinf(red_phase)) * 255.0) as u8;
            let green = ((libm::sinf(green_phase)) * 255.0) as u8;
            let blue = ((libm::sinf(blue_phase)) * 255.0) as u8;

            colors[led_index] = RGB8::new(red, green, blue);
        }

        if let Err(e) = neopixel.write(colors) {
            println!("Failed to write colors: {:?}", e);
        }
        i += 1;

        if started.elapsed().as_secs() > 5 {
            break;
        }
    }
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) -> ! {
    println!("Hello, world!");

    match _main(spawner).await {
        Err(e) => {
            println!("Error!");
            println!("{:?}", e);
            loop {}
        }
    }
}

async fn _main(_spawner: Spawner) -> Result<!> {
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let peripherals: Peripherals = esp_hal::init(esp_hal::Config::default());

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let timer0: AnyTimer = timg0.timer0.into();
    let timer1: AnyTimer = timg0.timer1.into();
    esp_hal_embassy::init([timer0, timer1]);

    let _delay = Delay::new();

    let mut cpu_control = CpuControl::new(peripherals.CPU_CTRL);

    // Neopixel setup:
    //  DMA TX buffer size:
    //    256 LEDs * 3 bytes (r g b) * 4 (4 SPI bytes are used for one ws2812 byte) + 1 or 2 reset sequences of 140 bytes each
    //    2 * 140 + 256 * 3 * 4 = 3352
    //    ==> round up to 4 kB
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(1, 4 * 1024);
    let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer)
        .map_err(|err| error_with_location!("Failed to create DMA RX buffer: {:?}", err))?;
    let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer)
        .map_err(|err| error_with_location!("Failed to create DMA TX buffer: {:?}", err))?;

    let spi: esp_hal::spi::master::SpiDmaBus<'_, esp_hal::Blocking> =
        esp_hal::spi::master::Spi::new(
            peripherals.SPI2,
            esp_hal::spi::master::Config::default().with_frequency(Rate::from_khz(4_500)),
        )?
        .with_mosi(peripherals.GPIO21)
        .with_dma(peripherals.DMA_CH1)
        .with_buffers(dma_rx_buf, dma_tx_buf);

    // start Neopixel task on the second core

    static NEOPIXEL_SIGNAL: StaticCell<Signal<CriticalSectionRawMutex, Box<[RGB8]>>> =
        StaticCell::new();
    let neopixel_signal = &*NEOPIXEL_SIGNAL.init(Signal::new());

    let _guard = cpu_control
        .start_app_core(unsafe { &mut *addr_of_mut!(APP_CORE_STACK) }, move || {
            static EXECUTOR: StaticCell<Executor> = StaticCell::new();
            let executor = EXECUTOR.init(Executor::new());
            executor.run(|spawner| {
                spawner.spawn(neopixel_task(spi, neopixel_signal)).ok();
            });
        })
        .unwrap();

    // // UART setup
    // let config = esp_hal::uart::Config::default().with_baudrate(115200);
    // let mut uart: Uart<'_, esp_hal::Blocking> = Uart::new(peripherals.UART1, config)?
    //     .with_rx(peripherals.GPIO17)
    //     .with_tx(peripherals.GPIO8);

    // I2S setup
    const I2S_BUFFER_SIZE: usize = 8 * 4092;
    let (mut rx_buffer, rx_descriptors, _, _) = dma_buffers!(I2S_BUFFER_SIZE, 0);
    let i2s = esp_hal::i2s::master::I2s::new(
        peripherals.I2S0,
        Standard::Philips,
        DataFormat::Data32Channel32,
        Rate::from_khz(48),
        peripherals.DMA_CH0,
    )
    .with_mclk(peripherals.GPIO0);

    let mut i2s_rx = i2s
        .i2s_rx
        .with_bclk(peripherals.GPIO4)
        .with_ws(peripherals.GPIO6)
        .with_din(peripherals.GPIO5)
        .build(rx_descriptors);

    let mut transfer = i2s_rx
        .read_dma_circular(&mut rx_buffer)
        .map_err(|err| error_with_location!("Failed to start I2S DMA transfer: {:?}", err))?;

    let mut i2s_buffer = [0u8; I2S_BUFFER_SIZE];
    loop {
        let available_i2s_bytes = transfer
            .available()
            .map_err(|err| error_with_location!("Failed to get available data: {:?}", err))?;

        const SAMPLE_SIZE: usize = 4 * 2; // 2 * 24 bit stereo in 32-bit containers
        const SAMPLES_TO_TAKE: usize = 256;

        if available_i2s_bytes >= SAMPLES_TO_TAKE * SAMPLE_SIZE {
            transfer.pop(&mut i2s_buffer).map_err(|err| {
                error_with_location!("Failed to pop data from transfer: {:?}", err)
            })?;

            // we copied over the whole DMA buffer, let's take the newest 256 samples
            let start_index = available_i2s_bytes - (SAMPLES_TO_TAKE * SAMPLE_SIZE);
            let slice = &i2s_buffer[start_index..available_i2s_bytes];
            match process_audio_samples(slice) {
                Ok((left_samples, _right_samples)) => {
                    assert!(left_samples.len() == SAMPLES_TO_TAKE);
                    let color_data = process_fft(&left_samples);
                    neopixel_signal.signal(color_data);
                }
                Err(e) => {
                    println!("Audio processing error: {:?}", e);
                }
            }
        }
    }
}

fn process_audio_samples(
    buffer: &[u8],
) -> Result<(heapless::Vec<i32, 512>, heapless::Vec<i32, 512>)> {
    if buffer.len() % 8 != 0 {
        return Err(error_with_location!(
            "Buffer length must be a multiple of 8"
        ));
    }

    let mut left_samples = heapless::Vec::new();
    let mut right_samples = heapless::Vec::new();

    for chunk in buffer.chunks_exact(8) {
        let left_value = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let _ = left_samples.push(left_value);

        let right_value = i32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let _ = right_samples.push(right_value);
    }

    Ok((left_samples, right_samples))
}

fn hann_window(buffer: &mut [f32]) {
    let n = buffer.len();
    if n == 0 {
        return;
    }
    let denom = (n - 1) as f32;
    for (i, v) in buffer.iter_mut().enumerate() {
        // Hann window: w[n] = 0.5 * (1 - cos(2π n / (N-1)))
        let phase = (i as f32) / denom;
        let w = 0.5 * (1.0 - libm::cosf(2.0 * core::f32::consts::PI * phase));
        *v *= w;
    }
}
//

fn process_fft(samples: &[i32]) -> Box<[RGB8]> {
    // static mut LAST_PRINT: u64 = 0;
    // static mut PROGRAM_START: Option<esp_hal::time::Instant> = None;
    // let program_start = unsafe {
    //     if matches!(PROGRAM_START, None) {
    //         PROGRAM_START = Some(esp_hal::time::Instant::now());
    //     }
    //     PROGRAM_START.unwrap()
    // };
    // let function_start = program_start.elapsed().as_millis();

    // Take up to 512 samples, pad with zeros if needed
    let mut fft_input = [0.0f32; 512];
    let sample_count = core::cmp::min(samples.len(), 512);
    let padding_count = 512 - sample_count;
    let left_padding = padding_count / 2;
    let _right_padding = padding_count - left_padding;

    // Normalize and copy samples
    const MAX_VALUE: f32 = (1 << 23) as f32;
    for (i, &sample) in samples.iter().take(sample_count).enumerate() {
        fft_input[left_padding + i] = (sample as f32) / MAX_VALUE;
    }

    // apply window to the populated region before FFT
    // hann_window(&mut fft_input[left_padding..left_padding + sample_count]);

    // Perform FFT
    let spectrum = rfft_512(&mut fft_input);

    // Group frequency bands (48kHz/512 = ~94Hz per bin)
    // Low: bin 1 (~94Hz)
    // Mid: bins 2-6 (~188-564Hz)
    // Treble: bins 7-15 (~658-1410Hz)
    // High: bins 16-25 (~1504-2350Hz)
    fn norm_bucket(c: &Complex32) -> f32 {
        // variant 1: linear
        // libm::sqrtf(c.norm_sqr()) / 512.0

        // variant 2: squared
        // c.norm_sqr() * 0.001 / 255.0

        // variant 3: squared + noise gate + x^3
        // const GATE: f32 = 0.3;
        // let val = c.norm_sqr() * 0.001 / 255.0;
        // if val < GATE {
        //     0.0
        // } else {
        //     // scale to 1.0
        //     let val = val;
        //     val * val * val
        // }

        // // variant 4: premult + squared + noise gate + x^3
        const GATE: f32 = 0.01;
        let c = c.scale(3.0);
        let val = c.norm_sqr() * 0.001 / 255.0;
        if val < GATE {
            0.0
        } else {
            // scale to 1.0
            let val = val;
            val * val * val
        }
    }

    // float between 0.0 and 1.0
    let bass_energy: f32 = spectrum[1..2].iter().map(|c| norm_bucket(c)).sum();
    let mid_energy: f32 = spectrum[2..11].iter().map(|c| norm_bucket(c)).sum();
    let treble_energy: f32 = spectrum[11..16].iter().map(|c| norm_bucket(c)).sum();
    let high_energy: f32 = spectrum[16..26].iter().map(|c| norm_bucket(c)).sum();

    // Int between 0 and 255
    let low = (bass_energy * 255.0).min(255.0) as u8;
    let mid = (mid_energy * 255.0).min(255.0) as u8;
    let treble = (treble_energy * 255.0).min(255.0) as u8;
    let high = (high_energy * 255.0).min(255.0) as u8;

    // Color
    let low_color = RGB8::new(low, 0, 0); // Red for low frequencies
    let mid_color = RGB8::new(0, mid, 0); // Green for mid frequencies
    let treble_color = RGB8::new(0, 0, treble); // Blue for treble frequencies
    let high_color = RGB8::new(high, high, high); // White for high frequencies

    // 16x16 panel (256 LEDs total)
    let mut colors = [RGB8::new(0, 0, 0); MATRIX_LENGTH];

    // create a striped pattern, with 8-pixel stripes
    for i in 0..256 {
        let row = i / 16;
        let col = i % 16;

        colors[i] = if row < 8 && col < 8 {
            low_color
        } else if row < 8 && col >= 8 {
            mid_color
        } else if row >= 8 && col < 8 {
            treble_color
        } else {
            high_color
        };
    }

    Box::new(colors)
}
