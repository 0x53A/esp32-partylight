use alloc::{boxed::Box, format};
use common::config::AppConfig;
use common::config::ChannelConfig;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

use esp_hal::Async;
use esp_hal::{dma_buffers, i2s::master::DataFormat, time::Rate};

use anyhow::{Result};

use microfft::{Complex32, real::rfft_512};
use smart_leds::RGB8;

use crate::error_with_location;
use crate::static_buf;
use crate::ws2812::WS2812_RESET_BYTES;
use crate::ws2812::WS2812_Spi;

#[cfg(feature = "fake-i2s")]
static FAKE_AUDIO_DATA: &[u8] = include_bytes!("../../test_audio_adpcm.wav");

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

/// Audio processing task for USB audio input
#[embassy_executor::task]
pub async fn usb_audio_processing_task(
    audio_buffer_receiver: &'static embassy_sync::channel::Receiver<
        'static,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        Box<[u8; 2048]>,
        4,
    >,
    neopixel_signal: &'static Signal<CriticalSectionRawMutex, Box<[RGB8; TOTAL_NEOPIXEL_LENGTH]>>,
    config_signal: &'static Signal<CriticalSectionRawMutex, AppConfig>,
) -> ! {
    let mut current_config = config_signal.wait().await;
    log::info!("USB audio processing task started");

    loop {
        // Check for config updates
        if let Some(new_config) = config_signal.try_take() {
            log::info!("Received updated config");
            current_config = new_config;
        }

        // Wait for audio data from USB
        let buffer = audio_buffer_receiver.receive().await;

        const SAMPLE_SIZE: usize = 4 * 2; // 2 * 32-bit stereo samples
        const SAMPLES_TO_TAKE: usize = 256;

        if buffer.len() >= SAMPLES_TO_TAKE * SAMPLE_SIZE {
            let slice = &buffer[0..SAMPLES_TO_TAKE * SAMPLE_SIZE];
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

#[cfg(feature = "fake-i2s")]
struct WaveHeader {
    sample_rate: u32,
    bits_per_sample: u16,
    num_channels: u16,
    data_offset: usize,
    data_size: usize,
    audio_format: u16, // 1 = PCM, 17 = IMA ADPCM
    block_align: u16,
}

#[cfg(feature = "fake-i2s")]
struct AdpcmDecoder {
    predictor: [i32; 2],  // One per channel
    step_index: [i32; 2], // One per channel
}

#[cfg(feature = "fake-i2s")]
impl AdpcmDecoder {
    fn new() -> Self {
        Self {
            predictor: [0, 0],
            step_index: [0, 0],
        }
    }

    fn decode_block(&mut self, input: &[u8], output: &mut [i32], num_channels: usize) -> usize {
        if input.len() < 4 * num_channels {
            return 0;
        }

        const STEP_TABLE: [i32; 89] = [
            7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45,
            50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230,
            253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963,
            1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327,
            3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487,
            12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
        ];

        const INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

        let mut out_pos = 0;
        let samples_per_block = if num_channels == 2 { 
            ((input.len() - 8) * 2) + 2 
        } else { 
            ((input.len() - 4) * 2) + 1 
        };

        // Decode initial predictors
        for ch in 0..num_channels {
            let offset = ch * 4;
            self.predictor[ch] = i16::from_le_bytes([input[offset], input[offset + 1]]) as i32;
            self.step_index[ch] = input[offset + 2] as i32;
            
            if self.step_index[ch] > 88 {
                self.step_index[ch] = 88;
            }

            // Output first sample (the predictor itself)
            if out_pos < output.len() {
                output[out_pos] = self.predictor[ch] << 16; // Scale to 32-bit
                out_pos += 1;
            }
        }

        // Decode nibbles
        let data_start = num_channels * 4;
        for i in (data_start..input.len()).step_by(num_channels * 4) {
            for ch in 0..num_channels {
                if i + ch * 4 + 3 >= input.len() {
                    break;
                }

                for byte_idx in 0..4 {
                    if i + ch * 4 + byte_idx >= input.len() {
                        break;
                    }
                    
                    let byte = input[i + ch * 4 + byte_idx];
                    
                    // Process low nibble
                    if out_pos < output.len() {
                        let nibble = (byte & 0x0F) as i32;
                        let step = STEP_TABLE[self.step_index[ch] as usize];
                        
                        let mut diff = step >> 3;
                        if nibble & 4 != 0 { diff += step; }
                        if nibble & 2 != 0 { diff += step >> 1; }
                        if nibble & 1 != 0 { diff += step >> 2; }
                        
                        if nibble & 8 != 0 {
                            self.predictor[ch] -= diff;
                        } else {
                            self.predictor[ch] += diff;
                        }
                        
                        self.predictor[ch] = self.predictor[ch].clamp(-32768, 32767);
                        self.step_index[ch] += INDEX_TABLE[nibble as usize];
                        self.step_index[ch] = self.step_index[ch].clamp(0, 88);
                        
                        output[out_pos] = self.predictor[ch] << 16; // Scale to 32-bit
                        out_pos += 1;
                    }
                    
                    // Process high nibble
                    if out_pos < output.len() {
                        let nibble = ((byte >> 4) & 0x0F) as i32;
                        let step = STEP_TABLE[self.step_index[ch] as usize];
                        
                        let mut diff = step >> 3;
                        if nibble & 4 != 0 { diff += step; }
                        if nibble & 2 != 0 { diff += step >> 1; }
                        if nibble & 1 != 0 { diff += step >> 2; }
                        
                        if nibble & 8 != 0 {
                            self.predictor[ch] -= diff;
                        } else {
                            self.predictor[ch] += diff;
                        }
                        
                        self.predictor[ch] = self.predictor[ch].clamp(-32768, 32767);
                        self.step_index[ch] += INDEX_TABLE[nibble as usize];
                        self.step_index[ch] = self.step_index[ch].clamp(0, 88);
                        
                        output[out_pos] = self.predictor[ch] << 16; // Scale to 32-bit
                        out_pos += 1;
                    }
                }
            }
        }

        out_pos
    }
}

#[cfg(feature = "fake-i2s")]
fn parse_wave_header(data: &[u8]) -> Result<WaveHeader> {
    if data.len() < 44 {
        return Err(error_with_location!("WAVE file too small"));
    }
    
    // Check RIFF header
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(error_with_location!("Invalid WAVE file header"));
    }
    
    // Find fmt chunk
    let mut offset = 12;
    while offset + 8 < data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;
        
        if chunk_id == b"fmt " {
            if chunk_size < 16 {
                return Err(error_with_location!("Invalid fmt chunk size"));
            }
            
            let audio_format = u16::from_le_bytes([data[offset + 8], data[offset + 9]]);
            let num_channels = u16::from_le_bytes([data[offset + 10], data[offset + 11]]);
            let sample_rate = u32::from_le_bytes([
                data[offset + 12],
                data[offset + 13],
                data[offset + 14],
                data[offset + 15],
            ]);
            let block_align = u16::from_le_bytes([data[offset + 20], data[offset + 21]]);
            let bits_per_sample = u16::from_le_bytes([data[offset + 22], data[offset + 23]]);
            
            // Find data chunk
            let mut data_offset = offset + 8 + chunk_size;
            while data_offset + 8 < data.len() {
                let data_chunk_id = &data[data_offset..data_offset + 4];
                let data_chunk_size = u32::from_le_bytes([
                    data[data_offset + 4],
                    data[data_offset + 5],
                    data[data_offset + 6],
                    data[data_offset + 7],
                ]) as usize;
                
                if data_chunk_id == b"data" {
                    return Ok(WaveHeader {
                        sample_rate,
                        bits_per_sample,
                        num_channels,
                        data_offset: data_offset + 8,
                        data_size: data_chunk_size,
                        audio_format,
                        block_align,
                    });
                }
                
                data_offset += 8 + data_chunk_size;
            }
            
            return Err(error_with_location!("data chunk not found"));
        }
        
        offset += 8 + chunk_size;
    }
    
    Err(error_with_location!("fmt chunk not found"))
}

#[cfg(feature = "fake-i2s")]
fn read_fake_i2s_samples(
    buffer: &mut [u8],
    position: &mut usize,
    header: &WaveHeader,
    decoder: &mut AdpcmDecoder,
    decode_buffer: &mut [i32],
    decode_buffer_pos: &mut usize,
    decode_buffer_len: &mut usize,
) -> usize {
    let audio_data = &FAKE_AUDIO_DATA[header.data_offset..header.data_offset + header.data_size];
    let mut written = 0;
    
    // For ADPCM, we need to decode blocks
    if header.audio_format == 17 {
        // IMA ADPCM
        while written < buffer.len() {
            // If we have decoded samples available, use them
            if *decode_buffer_pos < *decode_buffer_len {
                let sample = decode_buffer[*decode_buffer_pos];
                *decode_buffer_pos += 1;
                
                // Convert i32 sample to 4 bytes (little-endian)
                let bytes = sample.to_le_bytes();
                let to_write = core::cmp::min(4, buffer.len() - written);
                buffer[written..written + to_write].copy_from_slice(&bytes[..to_write]);
                written += to_write;
            } else {
                // Need to decode more data
                let block_size = header.block_align as usize;
                if *position + block_size > audio_data.len() {
                    // Loop back to start
                    *position = 0;
                    decoder.predictor = [0, 0];
                    decoder.step_index = [0, 0];
                    continue;
                }
                
                let block = &audio_data[*position..*position + block_size];
                *decode_buffer_len = decoder.decode_block(block, decode_buffer, header.num_channels as usize);
                *decode_buffer_pos = 0;
                *position += block_size;
            }
        }
    } else {
        // PCM - direct copy
        while written < buffer.len() {
            let remaining_audio = audio_data.len() - *position;
            if remaining_audio == 0 {
                // Loop back to start
                *position = 0;
                continue;
            }
            
            let to_copy = core::cmp::min(buffer.len() - written, remaining_audio);
            buffer[written..written + to_copy].copy_from_slice(&audio_data[*position..*position + to_copy]);
            *position += to_copy;
            written += to_copy;
        }
    }
    
    written
}

#[embassy_executor::task]
pub async fn audio_processing_task(
    i2s_peripherals: I2sPeripherals<'static>,
    neopixel_signal: &'static Signal<CriticalSectionRawMutex, Box<[RGB8; TOTAL_NEOPIXEL_LENGTH]>>,
    config_signal: &'static Signal<CriticalSectionRawMutex, AppConfig>,
) -> ! {
    let mut current_config = config_signal.wait().await;

    const I2S_BUFFER_SIZE: usize = 16 * 4 * 1024;

    #[cfg(feature = "fake-i2s")]
    {
        log::info!("Using fake I2S audio from embedded WAVE file");
        
        // Parse WAVE file header
        let wave_header = match parse_wave_header(FAKE_AUDIO_DATA) {
            Ok(header) => {
                log::info!("WAVE file: {}Hz, {} bits, {} channels, format: {}", 
                    header.sample_rate, header.bits_per_sample, header.num_channels, header.audio_format);
                header
            }
            Err(e) => {
                log::error!("Failed to parse WAVE file: {:?}", e);
                // Create a dummy header with safe defaults
                WaveHeader {
                    sample_rate: 48000,
                    bits_per_sample: 4,
                    num_channels: 2,
                    data_offset: 44,
                    data_size: 0,
                    audio_format: 17,
                    block_align: 1024,
                }
            }
        };
        
        let i2s_buffer = static_buf!(u8, I2S_BUFFER_SIZE);
        let mut position = 0usize;
        let mut decoder = AdpcmDecoder::new();
        
        // Buffer for decoded ADPCM samples (enough for one block)
        const MAX_SAMPLES_PER_BLOCK: usize = 4096;
        let decode_buffer = static_buf!(i32, MAX_SAMPLES_PER_BLOCK);
        let mut decode_buffer_pos = 0usize;
        let mut decode_buffer_len = 0usize;
        
        loop {
            // Check for config updates
            if let Some(new_config) = config_signal.try_take() {
                log::info!("Received updated config");
                current_config = new_config;
            }
            
            const SAMPLE_SIZE: usize = 4 * 2; // 2 * 24 bit stereo in 32-bit containers
            const SAMPLES_TO_TAKE: usize = 256;
            
            // Read fake samples (handles ADPCM decoding internally)
            let bytes_read = read_fake_i2s_samples(
                i2s_buffer,
                &mut position,
                &wave_header,
                &mut decoder,
                decode_buffer,
                &mut decode_buffer_pos,
                &mut decode_buffer_len,
            );
            
            if bytes_read >= SAMPLES_TO_TAKE * SAMPLE_SIZE {
                let slice = &i2s_buffer[0..SAMPLES_TO_TAKE * SAMPLE_SIZE];
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
            
            // Simulate timing similar to real I2S
            embassy_time::Timer::after(embassy_time::Duration::from_millis(10)).await;
        }
    }
    
    #[cfg(not(feature = "fake-i2s"))]
    {
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
