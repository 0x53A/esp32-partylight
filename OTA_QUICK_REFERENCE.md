# Bluetooth OTA Quick Reference

## Service UUIDs

```
OTA Service:        c6e7a9f0-1b34-4c5d-8f6e-2a3b4c5d6e7f
OTA Control:        d7f8b0e1-2c45-5d6e-9f7a-3b4c5d6e7f80
OTA Hash:           a0e1f2c3-5d6e-7f80-91a2-b3c4d5e6f7a8
OTA Data:           e8f9c1d2-3d56-6e7f-a08b-4c5d6e7f8091
OTA Status:         f9d0e2c3-4e67-7f80-b19c-5d6e7f809102
```

## Commands (OTA Control)

```
0x01 - Begin OTA update
0x02 - Commit update (reboot)
0x03 - Abort update
```

## Status Codes (OTA Status)

```
0x00 - Idle
0x01 - In Progress
0x02 - Success
0x03 - Error
```

## Usage Example (Python with bleak)

```python
from bleak import BleakClient
import asyncio
import hashlib

async def ota_update(address, firmware_data):
    # Calculate SHA256 hash
    firmware_hash = hashlib.sha256(firmware_data).digest()
    
    async with BleakClient(address) as client:
        # Send expected hash
        await client.write_gatt_char(
            "a0e1f2c3-5d6e-7f80-91a2-b3c4d5e6f7a8",
            firmware_hash
        )
        
        # Begin OTA
        await client.write_gatt_char(
            "d7f8b0e1-2c45-5d6e-9f7a-3b4c5d6e7f80",
            bytes([0x01])
        )
        
        # Send data in chunks
        CHUNK_SIZE = 512
        for i in range(0, len(firmware_data), CHUNK_SIZE):
            chunk = firmware_data[i:i+CHUNK_SIZE]
            await client.write_gatt_char(
                "e8f9c1d2-3d56-6e7f-a08b-4c5d6e7f8091",
                chunk
            )
            await asyncio.sleep(0.05)  # Small delay
        
        # Commit
        await client.write_gatt_char(
            "d7f8b0e1-2c45-5d6e-9f7a-3b4c5d6e7f80",
            bytes([0x02])
        )
```

## Usage Example (Rust/WASM)

```rust
// In web app
use js_sys::Uint8Array;
use web_sys::crypto;

async fn perform_ota(bt: &Bluetooth, firmware: Vec<u8>) -> Result<(), JsValue> {
    // Calculate SHA256 hash (using Web Crypto API)
    let hash_buffer = crypto::subtle::digest("SHA-256", &firmware).await?;
    let hash = Uint8Array::new(&hash_buffer);
    
    // Send hash
    bt.ota_set_hash(&hash).await?;
    
    // Begin
    bt.ota_begin().await?;
    
    // Send chunks
    for chunk in firmware.chunks(512) {
        let array = Uint8Array::from(chunk);
        bt.ota_write_chunk(&array).await?;
    }
    
    // Commit (device will verify hash)
    bt.ota_commit().await?;
    
    Ok(())
}
```

## CLI Tool

```bash
# Using the provided Python script
python mcu/ota_update.py firmware.bin [device_name]

# Example
python mcu/ota_update.py target/xtensa-esp32s3-none-elf/release/esp32_partylight.bin
```

## Testing

```bash
# 1. Build firmware
cd mcu
cargo build --release

# 2. Flash first time (via USB)
espflash flash target/xtensa-esp32s3-none-elf/release/esp32_partylight.bin

# 3. Update via Bluetooth
python ota_update.py target/xtensa-esp32s3-none-elf/release/esp32_partylight.bin
```

## Troubleshooting

| Problem | Solution |
|---------|----------|
| OTA service not found | Ensure device is running OTA-enabled firmware |
| Write failures | Reduce chunk size or increase delay |
| Device doesn't boot | Bootloader will auto-rollback after 3 attempts |
| Connection lost during update | Restart the update process |

## Important Notes

- Maximum chunk size: 512 bytes
- Update automatically aborted on disconnect
- Device reboots after successful commit
- Failed boot triggers automatic rollback
- No signature verification (add for production!)
