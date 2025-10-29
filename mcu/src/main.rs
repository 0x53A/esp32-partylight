#![no_std]
#![no_main]
#![feature(never_type)]
#![feature(generic_atomic)]
//#![feature(generic_const_exprs)]

extern crate alloc;
use alloc::{boxed::Box, format};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use esp_hal_embassy::Executor;
use log::{LevelFilter, info};

use core::{panic::PanicInfo, ptr::addr_of_mut};

use esp_hal::{
    delay::Delay,
    dma::{DmaRxBuf, DmaTxBuf},
    dma_buffers,
    rng::TrngSource,
    system::{CpuControl, Stack},
    time::Rate,
    timer::{AnyTimer, timg::TimerGroup},
};

use anyhow::{Result};

use esp_hal::peripherals::Peripherals;

use static_cell::StaticCell;

use smart_leds::RGB8;

use rtt_target::{ChannelMode, rprintln, rtt_init_print};

mod bluetooth;
mod lights;
pub mod util;
mod usb_audio;

mod ws2812;

use lights::*;

use util::*;

esp_bootloader_esp_idf::esp_app_desc!();

use esp_alloc as _;
// use esp_backtrace as _;

#[inline(never)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    rprintln!("{}", info);
    log::error!("{info}");

    loop {
        // prevent optimization
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

static mut APP_CORE_STACK: Stack<{ 8 * 1024 }> = Stack::new();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) -> ! {
    log::info!("Hello, world!");

    match _main(spawner).await {
        Err(e) => {
            log::error!("Error!");
            log::error!("{e:?}");
            loop {}
        }
    }
}

async fn _main(spawner: Spawner) -> Result<!> {
    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 64_000);

    // ---------------------------------------------------------------------------

    rtt_init_print!(ChannelMode::NoBlockTrim, 4 * 1024);

    static LOGGER: StaticCell<MultiLogger> = StaticCell::new();
    let logger = LOGGER.init(MultiLogger);

    log::set_logger(logger).map_err(|_| error_with_location!("Failed to set logger"))?;
    log::set_max_level(LevelFilter::Info);

    rprintln!("Hello, world!");
    log::info!("log::info");

    // ---------------------------------------------------------------------------

    let peripherals: Peripherals = esp_hal::init(esp_hal::Config::default()); // Note: 'default()' runs at 80 MHz (for the esp32-s3)

    // let neopixel_data_pin = peripherals.GPIO48; // internal single LED
    let neopixel_data_pin = peripherals.GPIO21; // external 16x16 matrix

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let timer0: AnyTimer = timg0.timer0.into();
    let timer1: AnyTimer = timg0.timer1.into();
    esp_hal_embassy::init([timer0, timer1]);

    let _delay = Delay::new();

    // declare signals for inter-task communication
    static CONFIG_SIGNAL: StaticCell<Signal<CriticalSectionRawMutex, common::config::AppConfig>> =
        StaticCell::new();
    let config_signal = &*CONFIG_SIGNAL.init(Signal::new());

    let initial_config = common::config::AppConfig::default();
    config_signal.signal(initial_config.clone());

    static NEOPIXEL_SIGNAL: StaticCell<
        Signal<CriticalSectionRawMutex, Box<[RGB8; TOTAL_NEOPIXEL_LENGTH]>>,
    > = StaticCell::new();
    let neopixel_signal = &*NEOPIXEL_SIGNAL.init(Signal::new());

    // Initialize RNG for Bluetooth and enable esp_preempt
    let _rng_source = TrngSource::new(peripherals.RNG, peripherals.ADC1);
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_preempt::start(timg1.timer0);

    // Start config processing task
    spawner
        .spawn(config_task(config_signal))
        .map_err(|e| error_with_location!("Failed to spawn config task: {:?}", e))?;

    // Start Bluetooth task
    info!("[main] Starting Bluetooth task ...");
    bluetooth::init_bluetooth(&spawner, peripherals.BT, config_signal, initial_config)
        .map_err(|e| error_with_location!("Failed to start Bluetooth task: {:?}", e))?;
    for _ in 0..10 {
        embassy_futures::yield_now().await;
    }
    info!("[main] Bluetooth task started");

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
        .with_mosi(neopixel_data_pin)
        .with_dma(peripherals.DMA_CH1)
        .with_buffers(dma_rx_buf, dma_tx_buf);

    // // UART setup
    // let config = esp_hal::uart::Config::default().with_baudrate(115200);
    // let mut uart: Uart<'_, esp_hal::Blocking> = Uart::new(peripherals.UART1, config)?
    //     .with_rx(peripherals.GPIO17)
    //     .with_tx(peripherals.GPIO8);

    // Choose between USB Audio or I2S input
    const USE_USB_AUDIO: bool = true;

    // I2S peripherals declaration (needed for both branches)
    let i2s_peripherals = if !USE_USB_AUDIO {
        Some(I2sPeripherals {
            i2s0: peripherals.I2S0,
            dma_ch0: peripherals.DMA_CH0,
            gpio0: peripherals.GPIO0,
            gpio4: peripherals.GPIO4,
            gpio6: peripherals.GPIO6,
            gpio5: peripherals.GPIO5,
        })
    } else {
        None
    };

    if USE_USB_AUDIO {
        // USB Audio setup
        log::info!("[main] Initializing USB Audio...");
        
        // Create a static channel for passing audio data from USB to audio processing
        use embassy_sync::channel;
        type AudioChannel = channel::Channel<
            embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
            Box<[u8; 2048]>,
            4,
        >;
        type AudioSender<'a> = channel::Sender<'a, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, Box<[u8; 2048]>, 4>;
        type AudioReceiver<'a> = channel::Receiver<'a, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, Box<[u8; 2048]>, 4>;
        
        static AUDIO_BUFFER_CHANNEL: StaticCell<AudioChannel> = StaticCell::new();
        static AUDIO_SENDER: StaticCell<AudioSender<'static>> = StaticCell::new();
        static AUDIO_RECEIVER: StaticCell<AudioReceiver<'static>> = StaticCell::new();
        
        let audio_channel = &*AUDIO_BUFFER_CHANNEL.init(channel::Channel::new());
        let audio_sender = AUDIO_SENDER.init(audio_channel.sender());
        let audio_receiver = AUDIO_RECEIVER.init(audio_channel.receiver());
        
        // ESP32-S3 USB OTG uses GPIO19 and GPIO20
        usb_audio::init_usb_audio(
            &spawner,
            peripherals.USB0,
            peripherals.GPIO20,
            peripherals.GPIO19,
            audio_sender,
        )
        .map_err(|e| error_with_location!("Failed to initialize USB audio: {:?}", e))?;
        
        // Start USB audio processing task
        spawner
            .spawn(lights::usb_audio_processing_task(
                audio_receiver,
                neopixel_signal,
                config_signal,
            ))
            .map_err(|e| error_with_location!("Failed to spawn USB audio processing task: {:?}", e))?;
        
        log::info!("[main] USB Audio initialized");
    }

    let mut cpu_control = CpuControl::new(peripherals.CPU_CTRL);
    let _guard = cpu_control
        .start_app_core(unsafe { &mut *addr_of_mut!(APP_CORE_STACK) }, move || {
            static EXECUTOR: StaticCell<Executor> = StaticCell::new();
            let executor = EXECUTOR.init(Executor::new());
            executor.run(|spawner| {
                // start Neopixel task
                spawner.spawn(neopixel_task(spi, neopixel_signal)).ok();

                // Start I2S audio processing task if not using USB audio
                if let Some(peripherals) = i2s_peripherals {
                    spawner
                        .spawn(audio_processing_task(
                            peripherals,
                            neopixel_signal,
                            config_signal,
                        ))
                        .ok();
                }
            });
        })
        .unwrap();

    // all processing is done in tasks
    loop {
        embassy_futures::yield_now().await;
    }
}
