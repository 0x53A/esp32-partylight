// https://github.com/embassy-rs/trouble/blob/main/examples/esp32/src/bin/ble_bas_peripheral_sec.rs

use common::config::AppConfig;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::select;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;
use esp_hal::peripherals::{BT, FLASH};
use esp_radio::ble::controller::BleConnector;
use log::{error, info, warn};
use rand_core::{CryptoRng, RngCore};
use trouble_host::prelude::*;

use crate::static_cell_init;

// OTA-related imports
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_storage::FlashStorage;
use sha2::{Digest, Sha256};

/// Max number of connections
const CONNECTIONS_MAX: usize = 1;

/// Max number of L2CAP channels.
const L2CAP_CHANNELS_MAX: usize = 2; // Signal + att

/// OTA control commands
const OTA_CMD_BEGIN: u8 = 0x01;
const OTA_CMD_COMMIT: u8 = 0x02;
const OTA_CMD_ABORT: u8 = 0x03;

/// OTA status codes
const OTA_STATUS_IDLE: u8 = 0x00;
const OTA_STATUS_IN_PROGRESS: u8 = 0x01;
const OTA_STATUS_SUCCESS: u8 = 0x02;
const OTA_STATUS_ERROR: u8 = 0x03;

/// OTA state
struct OtaState<'a> {
    ota_updater: Option<OtaUpdater<'a, FlashStorage<'a>>>,
    bytes_received: usize,
    expected_hash: Option<[u8; 32]>,
    hasher: Option<Sha256>,
}

// GATT Server definition
#[gatt_server]
struct Server {
    config_service: ConfigService,
    ota_service: OtaService,
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

/// OTA Service for firmware updates over Bluetooth
#[gatt_service(uuid = "c6e7a9f0-1b34-4c5d-8f6e-2a3b4c5d6e7f")]
struct OtaService {
    /// OTA Control characteristic - used to start, commit, or abort OTA
    /// Write: 0x01 = begin OTA, 0x02 = commit, 0x03 = abort
    #[descriptor(uuid = descriptors::CHARACTERISTIC_USER_DESCRIPTION, name = "ota_control", read, value = "OTA Control")]
    #[characteristic(uuid = "d7f8b0e1-2c45-5d6e-9f7a-3b4c5d6e7f80", write, read)]
    ota_control: u8,

    /// OTA Hash characteristic - expected SHA256 hash of firmware (32 bytes)
    #[descriptor(uuid = descriptors::CHARACTERISTIC_USER_DESCRIPTION, name = "ota_hash", read, value = "OTA Expected Hash")]
    #[characteristic(uuid = "a0e1f2c3-5d6e-7f80-91a2-b3c4d5e6f7a8", write, read)]
    ota_hash: heapless::Vec<u8, 32>,

    /// OTA Data characteristic - receives firmware data chunks
    #[descriptor(uuid = descriptors::CHARACTERISTIC_USER_DESCRIPTION, name = "ota_data", read, value = "OTA Data")]
    #[characteristic(uuid = "e8f9c1d2-3d56-6e7f-a08b-4c5d6e7f8091", write)]
    ota_data: heapless::Vec<u8, 512>,

    /// OTA Status characteristic - reports current status
    /// 0x00 = idle, 0x01 = in progress, 0x02 = success, 0x03 = error
    #[descriptor(uuid = descriptors::CHARACTERISTIC_USER_DESCRIPTION, name = "ota_status", read, value = "OTA Status")]
    #[characteristic(uuid = "f9d0e2c3-4e67-7f80-b19c-5d6e7f809102", read, notify)]
    ota_status: u8,
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
    flash: FLASH<'static>,
    config_signal: &Signal<CriticalSectionRawMutex, common::config::AppConfig>,
) -> Result<(), Error> {
    let config_version = &server.config_service.config_version;
    let config_data = &server.config_service.config_data;
    let ota_control = &server.ota_service.ota_control;
    let ota_hash = &server.ota_service.ota_hash;
    let ota_data = &server.ota_service.ota_data;
    let ota_status = &server.ota_service.ota_status;

    // Initialize flash storage and buffer for OTA (static lifetime)
    static mut FLASH_STORAGE: Option<FlashStorage<'static>> = None;
    static mut OTA_BUFFER: [u8; 3072] = [0u8; 3072];

    // Initialize flash storage on first use
    let flash_storage: &'static mut FlashStorage<'static> = unsafe {
        if FLASH_STORAGE.is_none() {
            FLASH_STORAGE = Some(FlashStorage::new(flash));
        }
        FLASH_STORAGE.as_mut().unwrap()
    };

    let buffer: &'static mut [u8; 3072] = unsafe { &mut OTA_BUFFER };

    // Initialize OTA state with 'static lifetime
    let mut ota_state: OtaState<'static> = OtaState {
        ota_updater: None,
        bytes_received: 0,
        expected_hash: None,
        hasher: None,
    };

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
                        } else if event.handle() == ota_control.handle {
                            let value = server.get(ota_control);
                            info!("[gatt] Read ota_control: {value:?}");
                        } else if event.handle() == ota_hash.handle {
                            let value = server.get(ota_hash);
                            info!("[gatt] Read ota_hash: {value:?}");
                        } else if event.handle() == ota_status.handle {
                            let value = server.get(ota_status);
                            info!("[gatt] Read ota_status: {value:?}");
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
                        } else if event.handle() == ota_control.handle {
                            let byte_data = event.data();
                            if byte_data.len() == 1 {
                                let cmd = byte_data[0];
                                info!("[gatt] OTA control command: {}", cmd);

                                match cmd {
                                    OTA_CMD_BEGIN => {
                                        info!("[ota] Beginning OTA update");
                                        match begin_ota(&mut ota_state, flash_storage, buffer) {
                                            Ok(_) => {
                                                server.set(ota_control, &OTA_CMD_BEGIN).ok();
                                                server
                                                    .set(ota_status, &OTA_STATUS_IN_PROGRESS)
                                                    .ok();
                                                info!("[ota] OTA update started successfully");
                                                None
                                            }
                                            Err(e) => {
                                                error!("[ota] Failed to begin OTA: {:?}", e);
                                                server.set(ota_status, &OTA_STATUS_ERROR).ok();
                                                Some(AttErrorCode::UNLIKELY_ERROR)
                                            }
                                        }
                                    }
                                    OTA_CMD_COMMIT => {
                                        info!("[ota] Committing OTA update");
                                        match commit_ota(&mut ota_state) {
                                            Ok(_) => {
                                                server.set(ota_control, &OTA_CMD_COMMIT).ok();
                                                server.set(ota_status, &OTA_STATUS_SUCCESS).ok();
                                                info!("[ota] OTA committed, system will restart");
                                                // Give time for response to be sent
                                                Timer::after_millis(100).await;
                                                esp_hal::reset::software_reset();
                                                None
                                            }
                                            Err(e) => {
                                                error!("[ota] Failed to commit OTA: {:?}", e);
                                                server.set(ota_status, &OTA_STATUS_ERROR).ok();
                                                Some(AttErrorCode::UNLIKELY_ERROR)
                                            }
                                        }
                                    }
                                    OTA_CMD_ABORT => {
                                        info!("[ota] Aborting OTA update");
                                        abort_ota(&mut ota_state);
                                        server.set(ota_control, &OTA_CMD_ABORT).ok();
                                        server.set(ota_status, &OTA_STATUS_IDLE).ok();
                                        None
                                    }
                                    _ => {
                                        warn!("[ota] Unknown OTA control command: {}", cmd);
                                        Some(AttErrorCode::VALUE_NOT_ALLOWED)
                                    }
                                }
                            } else {
                                warn!("[ota] Invalid OTA control data length");
                                Some(AttErrorCode::INVALID_ATTRIBUTE_VALUE_LENGTH)
                            }
                        } else if event.handle() == ota_hash.handle {
                            let byte_data = event.data();
                            if byte_data.len() == 32 {
                                let mut hash = [0u8; 32];
                                hash.copy_from_slice(byte_data);
                                ota_state.expected_hash = Some(hash);

                                // Update the characteristic value
                                server
                                    .set(ota_hash, &heapless::Vec::from_slice(byte_data).unwrap())
                                    .ok();

                                info!("[ota] Expected hash set: {:02x?}", &hash[..8]);
                                None
                            } else {
                                warn!(
                                    "[ota] Invalid hash length: expected 32 bytes, got {}",
                                    byte_data.len()
                                );
                                Some(AttErrorCode::INVALID_ATTRIBUTE_VALUE_LENGTH)
                            }
                        } else if event.handle() == ota_data.handle {
                            let byte_data = event.data();
                            info!("[ota] Received {} bytes of firmware data", byte_data.len());

                            match write_ota_data(&mut ota_state, byte_data) {
                                Ok(_) => {
                                    info!(
                                        "[ota] Wrote {} bytes (total: {})",
                                        byte_data.len(),
                                        ota_state.bytes_received
                                    );
                                    None
                                }
                                Err(e) => {
                                    error!("[ota] Failed to write OTA data: {:?}", e);
                                    server.set(ota_status, &OTA_STATUS_ERROR).ok();
                                    abort_ota(&mut ota_state);
                                    Some(AttErrorCode::UNLIKELY_ERROR)
                                }
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

    // Clean up OTA state on disconnect
    if ota_state.ota_updater.is_some() {
        warn!("[ota] Connection closed with OTA in progress, aborting");
        abort_ota(&mut ota_state);
    }

    info!("[gatt] disconnected: {reason:?}");
    Ok(())
}

/// Begin OTA update by initializing the OtaUpdater
fn begin_ota<'a>(
    ota_state: &mut OtaState<'a>,
    flash: &'a mut FlashStorage,
    buffer: &'a mut [u8; 3072],
) -> Result<(), &'static str> {
    if ota_state.ota_updater.is_some() {
        return Err("OTA already in progress");
    }

    // Check if hash is set
    if ota_state.expected_hash.is_none() {
        return Err("Expected hash not set");
    }

    info!("[ota] Beginning OTA update");

    // Create OtaUpdater - it will automatically select the next partition
    let ota_updater = match OtaUpdater::new(flash, buffer) {
        Ok(updater) => updater,
        Err(_) => return Err("Failed to create OtaUpdater"),
    };

    info!("[ota] OtaUpdater created successfully");

    ota_state.ota_updater = Some(ota_updater);
    ota_state.bytes_received = 0;
    ota_state.hasher = Some(Sha256::new());

    Ok(())
}

/// Write firmware data chunk to OTA partition
fn write_ota_data(ota_state: &mut OtaState, data: &[u8]) -> Result<(), &'static str> {
    let ota_updater = match ota_state.ota_updater.as_mut() {
        Some(updater) => updater,
        None => return Err("OTA not started"),
    };

    let hasher = match ota_state.hasher.as_mut() {
        Some(h) => h,
        None => return Err("Hasher not initialized"),
    };

    // Write data to OTA partition
    if let Err(_) = ota_updater.write(data) {
        return Err("Failed to write OTA data");
    }

    // Update hash with the data
    hasher.update(data);

    ota_state.bytes_received += data.len();

    Ok(())
}

/// Commit OTA update and mark it as bootable
fn commit_ota(ota_state: &mut OtaState) -> Result<(), &'static str> {
    let mut ota_updater = match ota_state.ota_updater.take() {
        Some(updater) => updater,
        None => return Err("OTA not started"),
    };

    let hasher = match ota_state.hasher.take() {
        Some(h) => h,
        None => return Err("Hasher not initialized"),
    };

    let expected_hash = match ota_state.expected_hash {
        Some(h) => h,
        None => return Err("Expected hash not set"),
    };

    info!(
        "[ota] Finalizing OTA update with {} bytes received",
        ota_state.bytes_received
    );

    // Calculate the actual hash
    let actual_hash = hasher.finalize();
    let actual_hash_bytes: [u8; 32] = actual_hash.into();

    // Compare hashes
    if actual_hash_bytes != expected_hash {
        error!("[ota] Hash mismatch!");
        error!("[ota] Expected: {:02x?}", expected_hash);
        error!("[ota] Actual:   {:02x?}", actual_hash_bytes);
        return Err("Hash validation failed");
    }

    info!("[ota] Hash validation successful");

    // Commit the update - this marks the new partition as bootable
    if let Err(_) = ota_updater.complete() {
        return Err("Failed to complete OTA update");
    }

    info!("[ota] OTA update completed successfully - restarting");

    // Trigger system reset
    esp_hal::reset::software_reset();

    #[allow(unreachable_code)]
    Ok(())
}

/// Abort OTA update and clean up
fn abort_ota(ota_state: &mut OtaState) {
    if let Some(mut ota_updater) = ota_state.ota_updater.take() {
        // Abort the update
        let _ = ota_updater.abort();
        info!("[ota] OTA update aborted");
    }
    ota_state.bytes_received = 0;
    ota_state.hasher = None;
    ota_state.expected_hash = None;
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
    flash: FLASH<'static>,
    config_signal: &'static Signal<CriticalSectionRawMutex, common::config::AppConfig>,
    initial_config: AppConfig,
) {
    info!("Bluetooth Task started");

    let radio = static_cell_init!(esp_radio::Controller<'static>, esp_radio::init().unwrap());

    let mut rng = esp_hal::rng::Trng::try_new().unwrap();

    let connector = BleConnector::new(radio, bt);
    let controller: ExternalController<_, 20> = ExternalController::new(connector);

    run(controller, &mut rng, flash, config_signal, initial_config).await;
}

pub fn init_bluetooth(
    spawner: &Spawner,
    bt: BT<'static>,
    flash: FLASH<'static>,
    config_signal: &'static Signal<CriticalSectionRawMutex, common::config::AppConfig>,
    initial_config: AppConfig,
) -> Result<(), embassy_executor::SpawnError> {
    spawner.spawn(bluetooth_task(bt, flash, config_signal, initial_config))
}
