use alloc::boxed::Box;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::zerocopy_channel;
use embassy_usb::class::uac1;
use embassy_usb::class::uac1::speaker::{self, Speaker, Volume};
use embassy_usb::driver::EndpointError;
use embassy_usb::{Builder, UsbDevice};
use esp_hal::otg_fs::{Usb, asynch::{Driver as UsbDriver, Config as UsbConfig}};
use esp_hal::peripherals;
use heapless::Vec;
use static_cell::StaticCell;
use core::sync::atomic::{Atomic, AtomicU32, Ordering};

use anyhow::Result;
use crate::error_with_location;

// Stereo input
pub const INPUT_CHANNEL_COUNT: usize = 2;

// Sample rate - match existing I2S configuration (48 kHz)
pub const SAMPLE_RATE_HZ: u32 = 48_000;

// Use 32 bit samples to match existing I2S processing
pub const SAMPLE_WIDTH: uac1::SampleWidth = uac1::SampleWidth::Width4Byte;
pub const SAMPLE_WIDTH_BIT: usize = SAMPLE_WIDTH.in_bit();
pub const SAMPLE_SIZE: usize = SAMPLE_WIDTH as usize;
pub const SAMPLE_SIZE_PER_S: usize = (SAMPLE_RATE_HZ as usize) * INPUT_CHANNEL_COUNT * SAMPLE_SIZE;

// Size of audio samples per 1 ms - for the full-speed USB frame period of 1 ms.
pub const USB_FRAME_SIZE: usize = SAMPLE_SIZE_PER_S.div_ceil(1000);

// Select front left and right audio channels.
pub const AUDIO_CHANNELS: [uac1::Channel; INPUT_CHANNEL_COUNT] = [
    uac1::Channel::LeftFront,
    uac1::Channel::RightFront,
];

// For ESP32-S3, use a more conservative packet size
// Full-speed USB typically supports up to 1023 bytes for isochronous endpoints
// But we'll use the actual frame size plus a small margin
pub const USB_MAX_PACKET_SIZE: usize = USB_FRAME_SIZE + 64; // ~384 + 64 = 448 bytes
pub const USB_MAX_SAMPLE_COUNT: usize = USB_MAX_PACKET_SIZE / SAMPLE_SIZE;

// Global volume state - store f32 bit pattern as u32
static VOLUME_LEFT: AtomicU32 = AtomicU32::new(0x3f800000); // 1.0f32 = full volume
static VOLUME_RIGHT: AtomicU32 = AtomicU32::new(0x3f800000); // 1.0f32 = full volume

fn volume_to_u32(volume: Volume) -> u32 {
    let f = match volume {
        Volume::Muted => 0.0f32,
        Volume::DeciBel(db) => {
            // Convert dB to linear scale: 10^(dB/20)
            libm::powf(10.0, db / 20.0)
        }
    };
    f.to_bits()
}

fn u32_to_scale(value: u32) -> f32 {
    f32::from_bits(value)
}

// The data type that is exchanged via the zero-copy channel (a sample vector).
pub type SampleBlock = Vec<u32, USB_MAX_SAMPLE_COUNT>;

// Feedback is provided in 10.14 format for full-speed endpoints.
pub const FEEDBACK_REFRESH_PERIOD: uac1::FeedbackRefresh = uac1::FeedbackRefresh::Period8Frames;

/// Apply volume scaling to a 32-bit sample
fn apply_volume(sample: u32, scale: f32) -> u32 {
    // Treat as signed 32-bit for proper audio scaling
    let signed = sample as i32;
    let scaled = (signed as f32 * scale) as i32;
    scaled as u32
}

struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}

/// Sends feedback messages to the host.
async fn feedback_handler<'d>(
    feedback: &mut speaker::Feedback<'d, UsbDriver<'d>>,
) -> Result<(), Disconnected> {
    let mut packet: Vec<u8, 4> = Vec::new();

    loop {
        // For ESP32-S3, we'll use a simpler fixed feedback approach
        // The feedback value tells the host how many samples we're consuming
        // For 48kHz with 10.14 format: 48 << 14 = 786432
        let feedback_value = (SAMPLE_RATE_HZ << 14) / 1000; // Per frame (1ms)
        
        packet.clear();
        packet.push(feedback_value as u8).unwrap();
        packet.push((feedback_value >> 8) as u8).unwrap();
        packet.push((feedback_value >> 16) as u8).unwrap();

        feedback.write_packet(&packet).await?;
        
        // Send feedback every FEEDBACK_REFRESH_PERIOD (8 frames = 8ms)
        embassy_time::Timer::after(embassy_time::Duration::from_millis(8)).await;
    }
}

/// Handles streaming of audio data from the host.
async fn stream_handler<'d>(
    stream: &mut speaker::Stream<'d, UsbDriver<'d>>,
    sender: &mut zerocopy_channel::Sender<'static, NoopRawMutex, SampleBlock>,
) -> Result<(), Disconnected> {
    loop {
        let mut usb_data = [0u8; USB_MAX_PACKET_SIZE];
        let data_size = stream.read_packet(&mut usb_data).await?;

        let word_count = data_size / SAMPLE_SIZE;

        if word_count * SAMPLE_SIZE == data_size {
            // Obtain a buffer from the channel
            let samples = sender.send().await;
            samples.clear();

            for w in 0..word_count {
                let byte_offset = w * SAMPLE_SIZE;
                let sample = u32::from_le_bytes(
                    usb_data[byte_offset..byte_offset + SAMPLE_SIZE]
                        .try_into()
                        .unwrap(),
                );

                // Fill the sample buffer with data.
                samples.push(sample).unwrap();
            }

            sender.send_done();
        } else {
            log::debug!("Invalid USB buffer size of {}, skipped.", data_size);
        }
    }
}

/// Receives audio samples from the USB streaming task and passes them to audio processing
#[embassy_executor::task]
pub async fn usb_audio_receiver_task(
    mut usb_audio_receiver: zerocopy_channel::Receiver<'static, NoopRawMutex, SampleBlock>,
    audio_buffer_sender: &'static embassy_sync::channel::Sender<
        'static,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        Box<[u8; 2048]>,
        4,
    >,
) {
    loop {
        let samples = usb_audio_receiver.receive().await;
        
        // Get current volume settings (stored as f32 bit patterns)
        let vol_left = VOLUME_LEFT.load(Ordering::Relaxed);
        let vol_right = VOLUME_RIGHT.load(Ordering::Relaxed);
        let scale_left = u32_to_scale(vol_left);
        let scale_right = u32_to_scale(vol_right);
        
        // USB audio samples are already interleaved stereo: [L, R, L, R, ...]
        // Each sample is a u32 (4 bytes)
        // Apply volume scaling and convert to bytes
        let mut buffer = Box::new([0u8; 2048]);
        let mut buffer_pos = 0;
        
        for (i, sample) in samples.iter().enumerate() {
            if buffer_pos + 4 <= buffer.len() {
                // Apply volume: left channel on even indices, right channel on odd
                let scale = if i % 2 == 0 { scale_left } else { scale_right };
                let scaled_sample = apply_volume(*sample, scale);
                
                let sample_bytes = scaled_sample.to_le_bytes();
                buffer[buffer_pos..buffer_pos + 4].copy_from_slice(&sample_bytes);
                buffer_pos += 4;
            } else {
                break;
            }
        }
        
        // Send to audio processing if we have data
        if buffer_pos > 0 {
            audio_buffer_sender.send(buffer).await;
        }

        // Notify the channel that the buffer is now ready to be reused
        usb_audio_receiver.receive_done();
    }
}

/// Receives audio samples from the host.
#[embassy_executor::task]
async fn usb_streaming_task(
    mut stream: speaker::Stream<'static, UsbDriver<'static>>,
    mut sender: zerocopy_channel::Sender<'static, NoopRawMutex, SampleBlock>,
) {
    loop {
        stream.wait_connection().await;
        log::info!("USB Audio stream connected");
        _ = stream_handler(&mut stream, &mut sender).await;
        log::info!("USB Audio stream disconnected");
    }
}

/// Sends sample rate feedback to the host.
#[embassy_executor::task]
async fn usb_feedback_task(
    mut feedback: speaker::Feedback<'static, UsbDriver<'static>>,
) {
    loop {
        feedback.wait_connection().await;
        log::info!("USB Audio feedback connected");
        _ = feedback_handler(&mut feedback).await;
        log::info!("USB Audio feedback disconnected");
    }
}

#[embassy_executor::task]
async fn usb_task(mut usb_device: UsbDevice<'static, UsbDriver<'static>>) {
    usb_device.run().await;
}

/// Checks for changes on the control monitor of the class.
///
/// In this case, monitor changes of volume or mute state.
#[embassy_executor::task]
async fn usb_control_task(control_monitor: speaker::ControlMonitor<'static>) {
    loop {
        control_monitor.changed().await;

        // Update volume for each channel
        if let Some(volume) = control_monitor.volume(uac1::Channel::LeftFront) {
            let volume_bits = volume_to_u32(volume);
            VOLUME_LEFT.store(volume_bits, Ordering::Relaxed);
            log::info!("Left volume changed to {:?} (scale: {:.3})", volume, u32_to_scale(volume_bits));
        }
        
        if let Some(volume) = control_monitor.volume(uac1::Channel::RightFront) {
            let volume_bits = volume_to_u32(volume);
            VOLUME_RIGHT.store(volume_bits, Ordering::Relaxed);
            log::info!("Right volume changed to {:?} (scale: {:.3})", volume, u32_to_scale(volume_bits));
        }
    }
}

pub fn init_usb_audio(
    spawner: &Spawner,
    usb0: peripherals::USB0<'static>,
    usb_dp: peripherals::GPIO20<'static>,
    usb_dm: peripherals::GPIO19<'static>,
    audio_buffer_sender: &'static embassy_sync::channel::Sender<
        'static,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        Box<[u8; 2048]>,
        4,
    >,
) -> Result<()> {
    log::info!("Initializing USB Audio...");

    // Configure all required buffers in a static way.
    log::debug!("USB packet size is {} bytes", USB_MAX_PACKET_SIZE);
    
    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    let config_descriptor = CONFIG_DESCRIPTOR.init([0; 256]);

    static BOS_DESCRIPTOR: StaticCell<[u8; 32]> = StaticCell::new();
    let bos_descriptor = BOS_DESCRIPTOR.init([0; 32]);

    const CONTROL_BUF_SIZE: usize = 64;
    static CONTROL_BUF: StaticCell<[u8; CONTROL_BUF_SIZE]> = StaticCell::new();
    let control_buf = CONTROL_BUF.init([0; CONTROL_BUF_SIZE]);

    // EP_OUT_BUFFER needs to be large enough for all OUT endpoints
    // Including control, feedback, and audio data endpoints
    // ESP32-S3 USB OTG needs more space for proper endpoint allocation
    // Using 4096 bytes to ensure we have enough space
    static EP_OUT_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUFFER.init([0u8; 4096]);

    static STATE: StaticCell<speaker::State> = StaticCell::new();
    let state = STATE.init(speaker::State::new());

    // Create the USB driver
    let usb = Usb::new(usb0, usb_dp, usb_dm);
    let usb_config = UsbConfig::default();
    let driver = UsbDriver::new(usb, ep_out_buffer, usb_config);

    // Basic USB device configuration
    let mut config = embassy_usb::Config::new(0x1209, 0x53A0 ); // https://github.com/pidcodes/pidcodes.github.com/pull/1111
    config.manufacturer = Some("Rieger Industries");
    config.product = Some("Diskomator 9000 Pro Max");
    config.serial_number = Some("0000000001");
    config.max_power = 500; // 500mA
    config.max_packet_size_0 = 64;

    let mut builder = Builder::new(
        driver,
        config,
        config_descriptor,
        bos_descriptor,
        &mut [], // no msos descriptors
        control_buf,
    );

    // Create the UAC1 Speaker class components
    let (stream, feedback, control_monitor) = Speaker::new(
        &mut builder,
        state,
        USB_MAX_PACKET_SIZE as u16,
        SAMPLE_WIDTH,
        &[SAMPLE_RATE_HZ],
        &AUDIO_CHANNELS,
        FEEDBACK_REFRESH_PERIOD,
    );

    // Create the USB device
    let usb_device = builder.build();

    // Establish a zero-copy channel for transferring received audio samples between tasks
    static SAMPLE_BLOCKS: StaticCell<[SampleBlock; 2]> = StaticCell::new();
    let sample_blocks = SAMPLE_BLOCKS.init([Vec::new(), Vec::new()]);

    static CHANNEL: StaticCell<zerocopy_channel::Channel<'_, NoopRawMutex, SampleBlock>> =
        StaticCell::new();
    let channel = CHANNEL.init(zerocopy_channel::Channel::new(sample_blocks));
    let (sender, receiver) = channel.split();

    // Launch USB audio tasks
    spawner
        .spawn(usb_control_task(control_monitor))
        .map_err(|_| error_with_location!("Failed to spawn usb_control_task"))?;
    spawner
        .spawn(usb_streaming_task(stream, sender))
        .map_err(|_| error_with_location!("Failed to spawn usb_streaming_task"))?;
    spawner
        .spawn(usb_feedback_task(feedback))
        .map_err(|_| error_with_location!("Failed to spawn usb_feedback_task"))?;
    spawner
        .spawn(usb_task(usb_device))
        .map_err(|_| error_with_location!("Failed to spawn usb_task"))?;
    spawner
        .spawn(usb_audio_receiver_task(receiver, audio_buffer_sender))
        .map_err(|_| error_with_location!("Failed to spawn usb_audio_receiver_task"))?;

    log::info!("USB Audio initialized successfully");
    Ok(())
}
