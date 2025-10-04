# ESP32 Partylight

LED matrix controller with audio visualization for ESP32-S3.

## Features

- 🎵 Real-time audio processing via I2S
- 💡 WS2812 LED control for 16x16 matrices
- 📱 Bluetooth Low Energy configuration interface
- 🔄 **Bluetooth OTA firmware updates** (NEW!)
- 🎨 Multiple visualization modes with customizable effects

## Bluetooth OTA Updates

This project now supports Over-The-Air (OTA) firmware updates via Bluetooth Low Energy, allowing wireless firmware updates without USB connection.

### Quick Start

1. **Build and flash initial firmware:**
   ```bash
   cd mcu
   cargo build --release
   espflash flash target/xtensa-esp32s3-none-elf/release/esp32_partylight.bin
   ```

2. **Perform OTA update:**
   ```bash
   python mcu/ota_update.py target/xtensa-esp32s3-none-elf/release/esp32_partylight.bin
   ```

### Documentation

- 📘 [OTA Architecture & Design](OTA_ARCHITECTURE.md) - Visual architecture and flow diagrams
- 📗 [Implementation Summary](BLUETOOTH_OTA_SUMMARY.md) - What was changed and how it works
- 📙 [Technical Documentation](mcu/OTA_README.md) - Detailed technical specifications
- 📕 [Quick Reference](OTA_QUICK_REFERENCE.md) - UUIDs, commands, and code examples

### OTA Service

The firmware exposes a Bluetooth GATT service for OTA updates:

- **Service UUID:** `c6e7a9f0-1b34-4c5d-8f6e-2a3b4c5d6e7f`
- **Characteristics:**
  - Control (write): Start, commit, or abort updates
  - Data (write): Receive firmware chunks
  - Status (read/notify): Monitor update progress

See [OTA_QUICK_REFERENCE.md](OTA_QUICK_REFERENCE.md) for complete API details.

## Project Structure

```
esp32-partylight/
├── mcu/                    # MCU firmware (Rust, no_std)
│   ├── src/
│   │   ├── main.rs
│   │   ├── bluetooth.rs   # BLE service with OTA
│   │   ├── lights.rs      # LED control
│   │   └── ws2812.rs      # WS2812 driver
│   ├── partitions.csv     # OTA partition layout
│   └── ota_update.py      # OTA update script
├── app/                    # Web configuration app (WASM)
│   └── src/
│       └── web_bluetooth.rs  # BLE client with OTA
├── common/                 # Shared configuration types
└── case/                   # 3D printable enclosure
```

## Building

### MCU Firmware

Requires the ESP Rust toolchain:

```bash
cd mcu
cargo build --release
```

### Web App

```bash
cd app
trunk build --release
```

## Hardware Requirements

- ESP32-S3 DevKit
- WS2812B LED matrix (16x16 recommended)
- I2S microphone (e.g., INMP441)

## Configuration

The device can be configured via:
1. Bluetooth using the web app or mobile app
2. Direct modification of `common/src/config.rs`

Configuration includes:
- LED brightness and color correction
- Audio processing parameters
- Visualization effects and modes

## Security Note

⚠️ The OTA implementation is designed for development and trusted environments. For production use, consider adding:
- Firmware signature verification
- BLE pairing/bonding
- Encrypted data transfer
- Version checking and rollback protection

## License

[Add your license here]

## Contributing

Contributions are welcome! Please feel free to submit pull requests.

## Acknowledgments

Built with:
- [esp-hal](https://github.com/esp-rs/esp-hal) - ESP32 hardware abstraction layer
- [embassy](https://github.com/embassy-rs/embassy) - Async embedded framework
- [trouble](https://github.com/embassy-rs/trouble) - Bluetooth Low Energy stack
