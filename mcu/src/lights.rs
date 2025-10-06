use alloc::{boxed::Box, format};
use common::config::AppConfig;
use common::config::ChannelConfig;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

use esp_hal::Async;
use esp_hal::{dma_buffers, i2s::master::DataFormat, time::Rate};

use anyhow::{Result, anyhow};

use microfft::{Complex32, real::rfft_512};
use smart_leds::RGB8;

use crate::error_with_location;
use crate::static_buf;
use crate::ws2812::WS2812_RESET_BYTES;
use crate::ws2812::WS2812_Spi;

const MATRIX_LENGTH: usize = 16 * 16;
const MATRIX_WIDTH: usize = 16;
pub const TOTAL_NEOPIXEL_LENGTH: usize = MATRIX_LENGTH;

const NEOPIXEL_MATRIX_BUFFER_SIZE: usize = 12 * TOTAL_NEOPIXEL_LENGTH + WS2812_RESET_BYTES;

#[embassy_executor::task]
pub async fn neopixel_task(
    spi: esp_hal::spi::master::SpiDmaBus<'static, esp_hal::Blocking>,
    pixel_signal: &'static Signal<CriticalSectionRawMutex, Box<[RGB8; TOTAL_NEOPIXEL_LENGTH]>>,
) -> ! {
    log::info!("Neopixel task started");

    // Note: this moves the buffer from the stack to a static location
    let neopixel_buffer = static_buf!(u8, NEOPIXEL_MATRIX_BUFFER_SIZE);

    let spi = spi.into_async();
    let mut neopixel = WS2812_Spi {
        spi,
        buffer: neopixel_buffer,
    };

    neopixel_demo(&mut neopixel).await;

    loop {
        let new_data = pixel_signal.wait().await;
        let write_result = neopixel
            .write_async(&new_data)
            .await
            .map_err(|err| error_with_location!("Failed to write to neopixel: {:?}", err));
        if let Err(e) = write_result {
            log::error!("{e:?}");
        }
    }
}

#[embassy_executor::task]
pub async fn config_task(_config_signal: &'static Signal<CriticalSectionRawMutex, AppConfig>) -> ! {
    loop {
        // let config = config_signal.wait().await;
        // log::info!("Received config update: {config:?}");

        // TODO: Parse config string and update application configuration
        // For now, just log it
        // Example: "fft_size:512,pattern:stripes,sample_count:256"

        embassy_futures::yield_now().await;
    }
}

async fn neopixel_demo(neopixel: &mut WS2812_Spi<'_, '_, Async, NEOPIXEL_MATRIX_BUFFER_SIZE>) {
    let started = esp_hal::time::Instant::now();
    let mut i = 0;
    loop {
        // Demo: Three sine waves cycling through the 16x16 matrix
        // Red starts at 0, Blue at 1/3, Green at 2/3 of the cycle
        let mut colors = [RGB8::new(0, 0, 0); TOTAL_NEOPIXEL_LENGTH];

        let time_offset = (i as f32) * 0.1; // Animation speed

        for led_index in 0..TOTAL_NEOPIXEL_LENGTH {
            let position =
                (led_index as f32) / TOTAL_NEOPIXEL_LENGTH as f32 * 2.0 * core::f32::consts::PI;

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

        if let Err(e) = neopixel.write_async(&colors).await {
            log::info!("Failed to write colors: {e:?}");
        }
        i += 1;

        if started.elapsed().as_secs() > 5 {
            break;
        }
    }
}

pub struct I2sPeripherals<'a> {
    pub i2s0: esp_hal::peripherals::I2S0<'a>,
    pub dma_ch0: esp_hal::peripherals::DMA_CH0<'a>,
    pub gpio0: esp_hal::peripherals::GPIO0<'a>, // MCLK
    pub gpio4: esp_hal::peripherals::GPIO4<'a>, // BCLK
    pub gpio6: esp_hal::peripherals::GPIO6<'a>, // WS
    pub gpio5: esp_hal::peripherals::GPIO5<'a>, // DIN
}

#[embassy_executor::task]
pub async fn audio_processing_task(
    i2s_peripherals: I2sPeripherals<'static>,
    neopixel_signal: &'static Signal<CriticalSectionRawMutex, Box<[RGB8; TOTAL_NEOPIXEL_LENGTH]>>,
    config_signal: &'static Signal<CriticalSectionRawMutex, AppConfig>,
) -> ! {
    let mut current_config = config_signal.wait().await;

    const I2S_BUFFER_SIZE: usize = 16 * 4 * 1024;

    let (mut rx_buffer, rx_descriptors, _, _) = dma_buffers!(I2S_BUFFER_SIZE, 0);

    let i2s = esp_hal::i2s::master::I2s::new(
        i2s_peripherals.i2s0,
        i2s_peripherals.dma_ch0,
        esp_hal::i2s::master::Config::new_tdm_philips()
            .with_sample_rate(Rate::from_khz(48))
            .with_data_format(DataFormat::Data32Channel32),
    )
    .unwrap()
    .with_mclk(i2s_peripherals.gpio0);

    let mut i2s_rx: esp_hal::i2s::master::I2sRx<'static, esp_hal::Blocking> = i2s
        .i2s_rx
        .with_bclk(i2s_peripherals.gpio4)
        .with_ws(i2s_peripherals.gpio6)
        .with_din(i2s_peripherals.gpio5)
        .build(rx_descriptors);

    let mut transfer = i2s_rx.read_dma_circular(&mut rx_buffer).unwrap(); // Handle error as appropriate

    let i2s_buffer = static_buf!(u8, I2S_BUFFER_SIZE);

    loop {
        // Check for config updates
        if let Some(new_config) = config_signal.try_take() {
            log::info!("Received updated config");
            current_config = new_config;
        }

        let available_i2s_bytes = match transfer.available() {
            Ok(bytes) => bytes,
            Err(err) => {
                panic!("Failed to get available data: {err:?}");
            }
        };

        const SAMPLE_SIZE: usize = 4 * 2; // 2 * 24 bit stereo in 32-bit containers
        const SAMPLES_TO_TAKE: usize = 256;

        if available_i2s_bytes >= SAMPLES_TO_TAKE * SAMPLE_SIZE {
            if let Err(err) = transfer.pop(i2s_buffer) {
                log::error!("Failed to pop data from transfer: {err:?}");
                embassy_futures::yield_now().await;
                continue;
            }

            // we copied over the whole DMA buffer, let's take the newest 256 samples
            let start_index = available_i2s_bytes - (SAMPLES_TO_TAKE * SAMPLE_SIZE);
            let slice = &i2s_buffer[start_index..available_i2s_bytes];
            match process_audio_samples(slice) {
                Ok((left_samples, _right_samples)) => {
                    assert!(left_samples.len() == SAMPLES_TO_TAKE);
                    let color_data = process_fft(&left_samples, &current_config);
                    neopixel_signal.signal(color_data);
                }
                Err(e) => {
                    log::error!("Audio processing error: {e:?}");
                }
            }
        }
        embassy_futures::yield_now().await;
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

fn process_fft(samples: &[i32], config: &AppConfig) -> Box<[RGB8; TOTAL_NEOPIXEL_LENGTH]> {
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

    // Normalize from signed 24-bit integer to -1.0..1.0 float and copy samples
    const MAX_VALUE: f32 = (1 << 23) as f32;
    for (i, &sample) in samples.iter().take(sample_count).enumerate() {
        fft_input[left_padding + i] = (sample as f32) / MAX_VALUE;
    }

    // apply window to the populated region before FFT
    if config.use_hann_window {
        hann_window(&mut fft_input[left_padding..left_padding + sample_count]);
    }

    // Perform FFT
    let spectrum = rfft_512(&mut fft_input);

    // 16x16 panel (256 LEDs total)
    let mut colors = [RGB8::new(0, 0, 0); MATRIX_LENGTH];

    fn calculate_channel(spectrum: &[Complex32], channel_cfg: &ChannelConfig) -> f32 {
        fn norm_one_bucket(c: &Complex32, channel_cfg: &ChannelConfig) -> f32 {
            // step 1: premult
            let c = c.scale(channel_cfg.premult);
            // step 2: from complex to real (squared, because that's faster)
            let val = c.norm_sqr() * 0.001 / 255.0;

            // step 3: noise gate
            if val < channel_cfg.noise_gate {
                return 0.0;
            }

            // step 4: exponent
            if channel_cfg.exponent == 1 {
                libm::sqrtf(val)
            } else if channel_cfg.exponent == 2 {
                val
            } else if channel_cfg.exponent % 2 == 0 {
                libm::powf(val, channel_cfg.exponent as f32 / 2.0)
            } else {
                libm::powf(libm::sqrtf(val), channel_cfg.exponent as f32)
            }
        }

        let buckets = spectrum[channel_cfg.start_index..=channel_cfg.end_index + 1]
            .iter()
            .map(|c| norm_one_bucket(c, channel_cfg));

        match channel_cfg.aggregate {
            common::config::AggregationMethod::Sum => buckets.sum::<f32>(),
            common::config::AggregationMethod::Max => buckets.reduce(f32::max).unwrap_or(0.0),
            common::config::AggregationMethod::Average => {
                let len = buckets.len() as f32;
                if len == 0.0 {
                    0.0
                } else {
                    buckets.sum::<f32>() / len
                }
            }
        }
    }

    match &config.pattern {
        common::config::NeopixelMatrixPattern::Stripes(channels) => {
            let channel_colors = channels.clone().map(|channel| {
                let f = calculate_channel(spectrum, &channel);
                let clamped = f.min(1.0);
                RGB8::new(
                    (clamped * channel.color[0] * 255.0) as u8,
                    (clamped * channel.color[1] * 255.0) as u8,
                    (clamped * channel.color[2] * 255.0) as u8,
                )
            });

            // create a striped pattern, with 8-pixel stripes
            for i in 0..256 {
                let row = i / 16;
                let col = i % 16;

                colors[i] = if row < 8 && col < 8 {
                    channel_colors[0]
                } else if row < 8 && col >= 8 {
                    channel_colors[1]
                } else if row >= 8 && col < 8 {
                    channel_colors[2]
                } else {
                    channel_colors[3]
                };
            }

            Box::new(colors)
        }
        common::config::NeopixelMatrixPattern::Bars(channels) => {
            let channel_strengths = channels.clone().map(|channel| {
                let f = calculate_channel(spectrum, &channel);

                f.min(1.0)
            });

            // create a bar pattern, with 2x16-pixel bars
            for i in 0..8 {
                let channel_cfg = &channels[i];
                let pixels = (channel_strengths[i] * 16.0) as usize;
                for y in 0..pixels {
                    for x in 0..2 {
                        let pixel_x = i * 2 + x;
                        let pixel_y = 15 - y; // bottom to top
                        let pixel = xy(&mut colors, pixel_x, pixel_y);
                        *pixel = RGB8::new(
                            (channel_strengths[i] * channel_cfg.color[0] * 255.0) as u8,
                            (channel_strengths[i] * channel_cfg.color[1] * 255.0) as u8,
                            (channel_strengths[i] * channel_cfg.color[2] * 255.0) as u8,
                        );
                    }
                }
            }

            Box::new(colors)
        }
        common::config::NeopixelMatrixPattern::Quarters(channels) => {
            let channel_colors = channels.clone().map(|channel| {
                let f = calculate_channel(spectrum, &channel);
                let clamped = f.min(1.0);
                RGB8::new(
                    (clamped * channel.color[0] * 255.0) as u8,
                    (clamped * channel.color[1] * 255.0) as u8,
                    (clamped * channel.color[2] * 255.0) as u8,
                )
            });

            // create a quartered pattern
            for i in 0..4 {
                for y in 0..8 {
                    for x in 0..8 {
                        let (offset_x, offset_y) = match i {
                            0 => (0, 0), // Top-left
                            1 => (8, 0), // Top-right
                            2 => (0, 8), // Bottom-left
                            3 => (8, 8), // Bottom-right
                            _ => (0, 0), // Should not happen
                        };
                        let pixel_x = offset_x + x;
                        let pixel_y = offset_y + y;
                        let pixel = xy(&mut colors, pixel_x, pixel_y);
                        *pixel = channel_colors[i];
                    }
                }
            }

            Box::new(colors)
        }
    }
}

/// Convert from x,y coordinates to the linear NeoPixel index
/// The XY coordinates are 0-indexed, with (0,0) at the top-left
/// x goes right, y goes down
fn xy<T>(arr: &mut [T], x: usize, y: usize) -> &mut T {
    // the strip starts at top left, goes down, then one right and up, one right and down, ...
    // so even columns go down, odd columns go up.
    let index = if x % 2 == 0 {
        // Even columns go down
        (x * MATRIX_WIDTH) + y
    } else {
        // Odd columns go up
        (x * MATRIX_WIDTH) + (MATRIX_WIDTH - 1 - y)
    };
    &mut arr[index]
}
