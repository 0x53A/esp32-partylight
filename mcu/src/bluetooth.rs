// https://github.com/embassy-rs/trouble/blob/main/examples/esp32/src/bin/ble_bas_peripheral_sec.rs

use common::config::AppConfig;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::select;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;
use esp_hal::peripherals::BT;
use esp_radio::ble::controller::BleConnector;
use log::{error, info, warn};
use rand_core::{CryptoRng, RngCore};
use trouble_host::prelude::*;

use crate::static_cell_init;

/// Max number of connections
const CONNECTIONS_MAX: usize = 1;

/// Max number of L2CAP channels.
const L2CAP_CHANNELS_MAX: usize = 2; // Signal + att

// GATT Server definition
#[gatt_server]
struct Server {
    config_service: ConfigService,
}

///
#[gatt_service(uuid = "bbafe0b7-bf3a-405a-bff7-d632c44c85f8")]
struct ConfigService {
    ///
    #[descriptor(uuid = descriptors::CHARACTERISTIC_USER_DESCRIPTION, name = "config_version", read, value = "Configuration Version")]
    #[characteristic(uuid = "ae1f519c-5884-489d-9cd4-4e3a0bf3d979", read, value = common::config::CONFIG_VERSION)]
    config_version: u32,

    #[descriptor(uuid = descriptors::CHARACTERISTIC_USER_DESCRIPTION, name = "config_data", read, value = "Configuration Data")]
    #[characteristic(uuid = "fa57339a-e7e0-434e-9c98-93a15061e1ff", write, read)]
    config_data: heapless::Vec<u8, 200>,
}

/// Run the BLE stack.
pub async fn run<C, RNG>(
    controller: C,
    random_generator: &mut RNG,
    config_signal: &Signal<CriticalSectionRawMutex, common::config::AppConfig>,
    initial_config: AppConfig,
) where
    C: Controller,
    RNG: RngCore + CryptoRng,
{
    // // Using a fixed "random" address can be useful for testing. In real scenarios, one would
    // // use e.g. the MAC 6 byte array as tshe address (how to get that varies by the platform).
    // let address: Address = Address::random([0xff, 0x8f, 0x1a, 0x05, 0xe4, 0xff]);
    // info!("Our address = {}", address);

    let mut resources: HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
        HostResources::new();
    let stack = trouble_host::new(controller, &mut resources)
        // .set_random_address(address)
        .set_random_generator_seed(random_generator);
    let Host {
        mut peripheral,
        runner,
        ..
    } = stack.build();

    info!("Starting advertising and GATT service");
    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: "Diskomator",
        appearance: &appearance::human_interface_device::GENERIC_HUMAN_INTERFACE_DEVICE,
    }))
    .unwrap();

    server
        .set(
            &server.config_service.config_data,
            &heapless::Vec::from_slice(initial_config.to_bytes::<200>().unwrap().as_slice())
                .unwrap(),
        )
        .unwrap();

    let _ = join(ble_task(runner), async {
        loop {
            match advertise("Diskomator", &mut peripheral, &server).await {
                Ok(conn) => {
                    // set up tasks when the connection is established to a central, so they don't run when no one is connected.
                    let a = gatt_events_task(&server, &conn, config_signal);
                    let b = custom_task(&server, &conn, &stack);
                    // run until any task ends (usually because the connection has been closed),
                    // then return to advertising state.
                    select(a, b).await;
                }
                Err(e) => {
                    error!("[adv] error: {e:?}");
                    panic!("[adv] error: {:?}", e);
                }
            }

            embassy_futures::yield_now().await;
        }
    })
    .await;
}

/// This is a background task that is required to run forever alongside any other BLE tasks.
///
/// ## Alternative
///
/// If you didn't require this to be generic for your application, you could statically spawn this with i.e.
///
/// ```rust,ignore
///
/// #[embassy_executor::task]
/// async fn ble_task(mut runner: Runner<'static, SoftdeviceController<'static>>) {
///     runner.run().await;
/// }
///
/// spawner.must_spawn(ble_task(runner));
/// ```
async fn ble_task<C: Controller, P: PacketPool>(mut runner: Runner<'_, C, P>) {
    loop {
        if let Err(e) = runner.run().await {
            error!("[ble_task] error: {e:?}");
            panic!("[ble_task] error: {:?}", e);
        }
        embassy_futures::yield_now().await;
    }
}

/// Stream Events until the connection closes.
///
/// This function will handle the GATT events and process them.
/// This is how we interact with read and write requests.
async fn gatt_events_task(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    config_signal: &Signal<CriticalSectionRawMutex, common::config::AppConfig>,
) -> Result<(), Error> {
    let config_version = &server.config_service.config_version;
    let config_data = &server.config_service.config_data;
    let reason = loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => break reason,
            // GattConnectionEvent::PairingComplete { security_level, .. } => {
            //     info!("[gatt] pairing complete: {:?}", security_level);
            // }
            // GattConnectionEvent::PairingFailed(err) => {
            //     error!("[gatt] pairing error: {:?}", err);
            // }
            GattConnectionEvent::Gatt { event } => {
                let result = match &event {
                    GattEvent::Read(event) => {
                        if event.handle() == config_version.handle {
                            let value = server.get(config_version);
                            info!("[gatt] Read config_version: {value:?}");
                        } else if event.handle() == config_data.handle {
                            let value = server.get(config_data);
                            info!("[gatt] Read config_data: {value:?}");
                        }
                        None
                    }
                    GattEvent::Write(event) => {
                        info!("[gatt] Write event: {:?}", event.handle());
                        if event.handle() == config_data.handle {
                            let byte_data = event.data();
                            info!(
                                "[gatt] Write to config_data with length {}",
                                byte_data.len()
                            );
                            if let Ok(new_config) = AppConfig::from_bytes(byte_data) {
                                info!("[gatt] Valid Data in config data");

                                // Signal the config update to other tasks
                                info!("[gatt] Signaling config update");
                                config_signal.signal(new_config);

                                // Update the characteristic value
                                server
                                    .set(
                                        config_data,
                                        &heapless::Vec::from_slice(byte_data).unwrap(),
                                    )
                                    .unwrap();

                                info!("[gatt] Updated config_data characteristic");
                                None
                            } else {
                                warn!("[gatt] Invalid Data in config data");
                                Some(AttErrorCode::VALUE_NOT_ALLOWED)
                            }
                        } else {
                            info!("[gatt] Write to unknown handle");
                            None
                        }
                    }
                    _ => None,
                };

                info!("[gatt] replying with {:?}", result);

                let reply_result = if let Some(code) = result {
                    event.reject(code)
                } else {
                    event.accept()
                };
                match reply_result {
                    Ok(reply) => reply.send().await,
                    Err(e) => warn!("[gatt] error sending response: {e:?}"),
                }
            }
            _ => {} // ignore other Gatt Connection Events
        }
        embassy_futures::yield_now().await;
    };
    info!("[gatt] disconnected: {reason:?}");
    Ok(())
}

/// Create an advertiser to use to connect to a BLE Central, and wait for it to connect.
async fn advertise<'values, 'server, C: Controller>(
    name: &'values str,
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    // Build advertising data (adv_data) and scan response (scan_data) separately.
    // Put the 128-bit service UUID in the advertising packet and the full local
    // name in the scan response to avoid exceeding the 31-byte adv payload.
    let mut adv_data = [0u8; 31];
    let mut scan_data = [0u8; 31];
    // UUID: bbafe0b7-bf3a-405a-bff7-d632c44c85f8 encoded as little-endian bytes
    let custom_uuid_le: [u8; 16] = [
        0xf8, 0x85, 0x4c, 0xc4, 0x32, 0xd6, 0xf7, 0xbf, 0x5a, 0x40, 0x3a, 0xbf, 0xb7, 0xe0, 0xaf,
        0xbb,
    ];

    let adv_len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::ServiceUuids128(&[custom_uuid_le]),
        ],
        &mut adv_data[..],
    )?;

    let scan_len = AdStructure::encode_slice(
        &[AdStructure::CompleteLocalName(name.as_bytes())],
        &mut scan_data[..],
    )?;

    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..adv_len],
                scan_data: &scan_data[..scan_len],
            },
        )
        .await?;
    info!("[adv] advertising");
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    info!("[adv] connection established");
    Ok(conn)
}

/// Example task to use the BLE notifier interface.
/// This task will notify the connected central of a counter value every 2 seconds.
/// It will also read the RSSI value every 2 seconds.
/// and will stop when the connection is closed by the central or an error occurs.
async fn custom_task<C: Controller, P: PacketPool>(
    _server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
    stack: &Stack<'_, C, P>,
) {
    loop {
        // read RSSI (Received Signal Strength Indicator) of the connection.
        if let Ok(rssi) = conn.raw().rssi(stack).await {
            info!("[custom_task] RSSI: {rssi:?}");
        } else {
            info!("[custom_task] error getting RSSI");
            break;
        };
        Timer::after_secs(2).await;
    }
}

#[embassy_executor::task]
async fn bluetooth_task(
    bt: BT<'static>,
    config_signal: &'static Signal<CriticalSectionRawMutex, common::config::AppConfig>,
    initial_config: AppConfig,
) {
    info!("Bluetooth Task started");

    let radio = static_cell_init!(esp_radio::Controller<'static>, esp_radio::init().unwrap());

    let mut rng = esp_hal::rng::Trng::try_new().unwrap();

    let connector = BleConnector::new(radio, bt);
    let controller: ExternalController<_, 20> = ExternalController::new(connector);

    run(controller, &mut rng, config_signal, initial_config).await;
}

pub fn init_bluetooth(
    spawner: &Spawner,
    bt: BT<'static>,
    config_signal: &'static Signal<CriticalSectionRawMutex, common::config::AppConfig>,
    initial_config: AppConfig,
) -> Result<(), embassy_executor::SpawnError> {
    spawner.spawn(bluetooth_task(bt, config_signal, initial_config))
}
