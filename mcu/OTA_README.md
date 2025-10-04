# Bluetooth OTA Updates

This document describes the Bluetooth Over-The-Air (OTA) update functionality added to the MCU firmware.

## Overview

The firmware now supports receiving firmware updates over Bluetooth Low Energy (BLE). This allows updating the device without physical access or a wired connection.

## Architecture

### Partition Layout

The partition table has been updated to support OTA:

- `nvs` - Non-volatile storage (24 KB)
- `phy_init` - PHY initialization data (4 KB)
- `otadata` - OTA data partition (8 KB) - tracks which partition is active
- `ota_0` - First application partition (1 MB)
- `ota_1` - Second application partition (1 MB)

The system uses a dual-partition scheme where one partition contains the running firmware and the other can be updated. After a successful update, the bootloader will boot from the newly updated partition.

## BLE Service

### Service UUID
`c6e7a9f0-1b34-4c5d-8f6e-2a3b4c5d6e7f`

### Characteristics

#### OTA Control (UUID: `d7f8b0e1-2c45-5d6e-9f7a-3b4c5d6e7f80`)
- **Type**: Write, Read
- **Description**: Controls OTA operations
- **Values**:
  - `0x01` - Begin OTA update
  - `0x02` - Commit update (marks new firmware as bootable and restarts)
  - `0x03` - Abort update

#### OTA Hash (UUID: `a0e1f2c3-5d6e-7f80-91a2-b3c4d5e6f7a8`)
- **Type**: Write, Read
- **Description**: Expected SHA256 hash of firmware (32 bytes)
- **Required**: Must be set before beginning OTA update
- **Validation**: Device compares calculated hash with expected hash before committing

#### OTA Data (UUID: `e8f9c1d2-3d56-6e7f-a08b-4c5d6e7f8091`)
- **Type**: Write only
- **Description**: Receives firmware data chunks (up to 512 bytes per write)

#### OTA Status (UUID: `f9d0e2c3-4e67-7f80-b19c-5d6e7f809102`)
- **Type**: Read, Notify
- **Description**: Reports current OTA status
- **Values**:
  - `0x00` - Idle (no update in progress)
  - `0x01` - In progress
  - `0x02` - Success
  - `0x03` - Error

## Update Procedure

1. **Connect** to the device via Bluetooth
2. **Calculate** SHA256 hash of the firmware binary
3. **Write** the 32-byte hash to OTA Hash characteristic
4. **Write** `0x01` to OTA Control characteristic to begin the update
5. **Write** firmware binary data in chunks to OTA Data characteristic
   - Maximum 512 bytes per chunk
   - Wait for each write to complete before sending the next chunk
6. **Write** `0x02` to OTA Control characteristic to commit the update
7. The device will:
   - Calculate the SHA256 hash of received firmware
   - Compare with the expected hash from step 3
   - If hashes match: mark firmware as bootable and reboot
   - If hashes don't match: reject the update and return error status

### Error Handling

- If an error occurs during the update, the OTA Status will change to `0x03`
- Write `0x03` to OTA Control to abort the update
- If the connection is lost during an update, the update is automatically aborted
- If the new firmware fails to boot, the bootloader will automatically roll back to the previous partition

## Web Client Support

The web application (`app/src/web_bluetooth.rs`) includes helper methods for OTA:

```rust
async fn ota_set_hash(hash: &Uint8Array) -> Result<(), JsValue>
async fn ota_begin() -> Result<(), JsValue>
async fn ota_write_chunk(data: &Uint8Array) -> Result<(), JsValue>
async fn ota_commit() -> Result<(), JsValue>
async fn ota_abort() -> Result<(), JsValue>
async fn ota_read_status() -> Result<u8, JsValue>
```

## Security Considerations

✅ **Hash validation** - Firmware integrity is verified using SHA256 hash before committing
- The OTA service is always advertised when Bluetooth is enabled
- No rollback protection is implemented (device can be downgraded)
- Consider adding BLE pairing/authentication for production use

## Example Usage (Pseudo-code)

```javascript
// Connect to device
await bluetooth.connect();

// Read firmware file
const firmware = await readFile("firmware.bin");

// Calculate SHA256 hash
const hashBuffer = await crypto.subtle.digest('SHA-256', firmware);
const hashArray = new Uint8Array(hashBuffer);

// Send hash before beginning OTA
await bluetooth.ota_set_hash(hashArray);

// Begin OTA
await bluetooth.ota_begin();

// Send in chunks
const CHUNK_SIZE = 512;
for (let offset = 0; offset < firmware.length; offset += CHUNK_SIZE) {
    const chunk = firmware.slice(offset, offset + CHUNK_SIZE);
    await bluetooth.ota_write_chunk(chunk);
    
    // Optionally check status
    const status = await bluetooth.ota_read_status();
    if (status === 0x03) { // Error
        throw new Error("OTA failed");
    }
}

// Commit and reboot
await bluetooth.ota_commit();
// Device will reboot here
```

## Building

The OTA-enabled firmware can be built with:

```bash
cd mcu
cargo build --release
```

The resulting binary will be in `target/xtensa-esp32s3-none-elf/release/esp32_partylight.bin`

## Troubleshooting

- **OTA service not found**: Ensure the device is running firmware with OTA support
- **Write failures**: Reduce chunk size or increase delay between writes
- **Device doesn't boot after update**: The bootloader will automatically roll back to the previous partition
- **Connection lost during update**: Start the process again - the update will be aborted automatically
