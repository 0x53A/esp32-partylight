# Bluetooth OTA Implementation Summary

This document summarizes the Bluetooth Over-The-Air (OTA) update implementation for the ESP32 Partylight project.

## What Was Added

### 1. Partition Table Changes (`mcu/partitions.csv`)

Changed from a single factory partition to a dual OTA partition layout:

```csv
# Before:
factory,  app,  factory, ,        1M,

# After:
otadata,  data, ota,     ,        0x2000,
ota_0,    app,  ota_0,   ,        1M,
ota_1,    app,  ota_1,   ,        1M,
```

This allows the ESP32 bootloader to support A/B partition updates with automatic rollback on boot failure.

### 2. Dependencies (`mcu/Cargo.toml`)

Added the ESP bootloader OTA library:

```toml
esp-bootloader-ota = { git = "https://github.com/esp-rs/esp-hal", features = ["esp32s3"] }
```

### 3. MCU Firmware (`mcu/src/bluetooth.rs`)

#### New BLE Service

Added an OTA service with UUID `c6e7a9f0-1b34-4c5d-8f6e-2a3b4c5d6e7f` containing:

- **OTA Control** (UUID: `d7f8b0e1-2c45-5d6e-9f7a-3b4c5d6e7f80`)
  - Write to control OTA operations (begin, commit, abort)
  
- **OTA Data** (UUID: `e8f9c1d2-3d56-6e7f-a08b-4c5d6e7f8091`)
  - Write firmware chunks (up to 512 bytes)
  
- **OTA Status** (UUID: `f9d0e2c3-4e67-7f80-b19c-5d6e7f809102`)
  - Read/notify current status (idle, in progress, success, error)

#### OTA Logic

Implemented functions for:
- `begin_ota()` - Initialize the next OTA partition
- `write_ota_data()` - Write firmware chunks to the partition
- `commit_ota()` - Finalize and mark as bootable, then reboot
- `abort_ota()` - Cancel the update and clean up

The GATT event handler was extended to process OTA characteristic writes and manage the update state.

### 4. Web Client (`app/src/web_bluetooth.rs`)

Extended the Bluetooth client with OTA support:

- Added OTA characteristic handles to the `Bluetooth` struct
- Updated connection logic to discover and cache OTA characteristics
- Implemented public methods for OTA operations:
  - `ota_begin()` - Start an update
  - `ota_write_chunk()` - Send firmware data
  - `ota_commit()` - Finalize the update
  - `ota_abort()` - Cancel the update
  - `ota_read_status()` - Check current status

### 5. Documentation

Created comprehensive documentation:

- **OTA_README.md** - Detailed technical documentation covering:
  - Architecture and partition layout
  - BLE service specification
  - Update procedure
  - Security considerations
  - Example usage

### 6. Testing Tools

Created a Python script (`ota_update.py`) for testing OTA updates:

```bash
# Usage
python ota_update.py firmware.bin [device_name]

# Example
python ota_update.py target/xtensa-esp32s3-none-elf/release/esp32_partylight.bin Blindomator
```

Features:
- Automatic device discovery
- Progress reporting
- Error handling and retry logic
- Status monitoring

## How It Works

### Update Flow

1. **Client connects** to the ESP32 via BLE
2. **Client sends** `0x01` (BEGIN) to OTA Control
3. **Device initializes** the inactive OTA partition
4. **Client sends** firmware in 512-byte chunks to OTA Data
5. **Device writes** each chunk to flash
6. **Client sends** `0x02` (COMMIT) to OTA Control
7. **Device validates** the firmware and marks it as bootable
8. **Device reboots** into the new firmware
9. **Bootloader verifies** the new firmware on first boot
10. If successful, the new firmware runs; if it fails, the bootloader automatically rolls back to the previous partition

### Error Recovery

- Connection loss during update → Update automatically aborted
- Write failure → Status changes to ERROR, client can retry or abort
- Boot failure → Bootloader automatically rolls back to previous firmware
- Manual abort → Client sends `0x03` (ABORT) to clean up

## Integration Points

### For Web App Developers

The web app can now perform OTA updates using the `Bluetooth` class:

```rust
// In your web app async code
let mut bt = Bluetooth::new();
bt.connect().await?;

// Start OTA
bt.ota_begin().await?;

// Send firmware in chunks
let firmware = /* read firmware binary */;
for chunk in firmware.chunks(512) {
    bt.ota_write_chunk(&Uint8Array::from(chunk)).await?;
}

// Commit and reboot
bt.ota_commit().await?;
```

### For Future Enhancements

Consider adding:
1. **Progress notifications** - Use the OTA Status characteristic's notify feature
2. **Firmware verification** - Add signature checking before commit
3. **Compression** - Compress firmware data to reduce transfer time
4. **Resume capability** - Store progress to resume interrupted updates
5. **Web UI** - Add an OTA update page to the web app

## File Changes Summary

```
mcu/
  ├── Cargo.toml                 # Added esp-bootloader-ota dependency
  ├── partitions.csv             # Changed to OTA partition layout
  ├── src/bluetooth.rs           # Added OTA service and logic
  ├── OTA_README.md              # Technical documentation
  └── ota_update.py              # Python testing script

app/
  └── src/web_bluetooth.rs       # Added OTA client support
```

## Testing Checklist

Before releasing this feature:

- [ ] Build firmware with new partition layout
- [ ] Flash to device using espflash/esptool
- [ ] Connect via Bluetooth and verify OTA service is discoverable
- [ ] Perform a test OTA update with a modified firmware binary
- [ ] Verify device reboots into new firmware
- [ ] Test rollback by flashing intentionally broken firmware
- [ ] Test abort functionality
- [ ] Test connection loss during update
- [ ] Verify web app can discover and use OTA characteristics

## Security Notes

⚠️ **Important**: The current implementation does NOT include:
- Firmware signature verification
- Encryption of firmware data
- Authentication of the OTA client

For production use, consider adding these security features to prevent:
- Unauthorized firmware updates
- Malicious firmware injection
- Man-in-the-middle attacks

## References

- ESP-IDF OTA Documentation: https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-reference/system/ota.html
- ESP-HAL OTA Example: https://github.com/esp-rs/esp-hal/tree/main/examples/ota
- Trouble BLE Stack: https://github.com/embassy-rs/trouble
