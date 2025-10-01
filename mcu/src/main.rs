#![no_std]
#![no_main]
#![feature(never_type)]

extern crate alloc;
use alloc::{boxed::Box, format};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use esp_hal_embassy::Executor;
use log::{LevelFilter, info};

use core::{panic::PanicInfo, ptr::addr_of_mut};

use esp_hal::{
    clock::CpuClock,
    delay::Delay,
    dma::{DmaRxBuf, DmaTxBuf},
    dma_buffers,
    rng::TrngSource,
    system::{CpuControl, Stack},
    time::Rate,
    timer::{AnyTimer, timg::TimerGroup},
};

use anyhow::{Result, anyhow};

use esp_hal::peripherals::Peripherals;

use static_cell::StaticCell;

use smart_leds::RGB8;

use rtt_target::{rprintln, rtt_init_print};

mod bluetooth;
mod lights;
pub mod util;

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

static mut APP_CORE_STACK: Stack<8192> = Stack::new();

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
    esp_alloc::heap_allocator!(size: 72 * 1024);

    // ---------------------------------------------------------------------------

    rtt_init_print!();

    static LOGGER: StaticCell<MultiLogger> = StaticCell::new();
    let logger = LOGGER.init(MultiLogger);

    log::set_logger(logger).map_err(|_| error_with_location!("Failed to set logger"))?;
    log::set_max_level(LevelFilter::Info);

    rprintln!("Hello, world!");
    log::info!("log::info");

    // ---------------------------------------------------------------------------

    let peripherals: Peripherals =
        esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz));

    // let neopixel_data_pin = peripherals.GPIO48; // internal single LED
    let neopixel_data_pin = peripherals.GPIO21; // external 16x16 matrix

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let timer0: AnyTimer = timg0.timer0.into();
    let timer1: AnyTimer = timg0.timer1.into();
    esp_hal_embassy::init([timer0, timer1]);

    let _delay = Delay::new();

    let mut cpu_control = CpuControl::new(peripherals.CPU_CTRL);

    // Initialize radio and RNG for Bluetooth
    let _rng_source = TrngSource::new(peripherals.RNG, peripherals.ADC1);
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_preempt::start(timg1.timer0);

    static CONFIG_SIGNAL: StaticCell<Signal<CriticalSectionRawMutex, common::config::AppConfig>> =
        StaticCell::new();
    let config_signal = &*CONFIG_SIGNAL.init(Signal::new());

    // Start Bluetooth task
    let initial_config = common::config::AppConfig::default();
    config_signal.signal(initial_config.clone());
    info!("[main] Starting Bluetooth task ...");
    bluetooth::init_bluetooth(&spawner, peripherals.BT, config_signal, initial_config)
        .map_err(|e| error_with_location!("Failed to start Bluetooth task: {:?}", e))?;
    for _ in 0..10 {
        embassy_futures::yield_now().await;
    }
    info!("[main] Bluetooth task started");

    // Start config processing task
    spawner
        .spawn(config_task(config_signal))
        .map_err(|e| error_with_location!("Failed to spawn config task: {:?}", e))?;

    // Neopixel setup:
    //  DMA TX buffer size:
    //    256 LEDs * 3 bytes (r g b) * 4 (4 SPI bytes are used for one ws2812 byte) + 1 or 2 reset sequences of 140 bytes each
    //    2 * 140 + 256 * 3 * 4 = 3352
    //    ==> round up to 4 kB
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(1, 16 * 1024);
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
    let i2s_peripherals = I2sPeripherals {
        i2s0: peripherals.I2S0,
        dma_ch0: peripherals.DMA_CH0,
        gpio0: peripherals.GPIO0,
        gpio4: peripherals.GPIO4,
        gpio6: peripherals.GPIO6,
        gpio5: peripherals.GPIO5,
    };

    // Start audio processing task
    spawner
        .spawn(audio_processing_task(
            i2s_peripherals,
            neopixel_signal,
            config_signal,
        ))
        .map_err(|e| error_with_location!("Failed to spawn audio processing task: {:?}", e))?;

    // all processing is done in tasks
    loop {
        embassy_futures::yield_now().await;
    }
}
