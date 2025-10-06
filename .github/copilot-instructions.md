# GitHub Copilot Instructions

## Project Overview

This is an ESP32-S3 based audio visualization system ("partylight") that analyzes audio input via FFT and drives NeoPixel LED strips with different patterns. The project includes:

- **MCU firmware** (Rust): Runs on ESP32-S3, handles audio processing, Bluetooth communication, and LED control
- **Configuration app** (Rust + WASM): Cross-platform app (Web, Windows, Android) using egui for configuring the device via Web Bluetooth
- **Common library**: Shared configuration types and serialization (using postcard)

## Repository Structure

```
├── app/          # Configuration app (egui + WASM/native)
├── mcu/          # ESP32-S3 firmware (esp-hal + embassy)
├── common/       # Shared types and configuration
└── case/         # 3D printable case files
```

## Development Environment Setup

### App (Configuration Tool)

**Prerequisites:**
```bash
# The app requires nightly Rust (configured via rust-toolchain file)
rustup toolchain install nightly
rustup target add --toolchain nightly wasm32-unknown-unknown

# Web build tool (one time)
cargo install --locked trunk

# Android target (one time, optional)
cargo install --git https://github.com/tauri-apps/cargo-mobile2
```

**Building and Testing:**
```bash
cd app

# Check/build
cargo check --no-default-features --features font_hack
cargo build --release

# Web development
trunk serve              # Debug mode at http://127.0.0.1:8080
trunk build --release    # Release build to /dist folder

# Desktop (Windows/macOS)
cargo run               # Debug
cargo build --release   # Release

# Android
cargo mobile init       # Once after clone to create /gen/ folder
cargo android run       # Debug
cargo android apk build --release  # Release builds
```

### MCU (ESP32-S3 Firmware)

**Prerequisites:**
```bash
# Install ESP Rust toolchain (configured via rust-toolchain.toml file)
# See https://github.com/esp-rs/rust-build and .github/workflows/pr-check.yml
# The CI uses esp-rs/xtensa-toolchain action for installation
```

**Building:**
```bash
cd mcu
cargo check --all-targets
cargo build --release
```

**Key Architecture:**
- Dual-core ESP32-S3: Core 0 handles Bluetooth, Core 1 handles audio processing
- Embassy async runtime for task management
- Bluetooth GATT server for configuration (using trouble-host)
- Audio input → FFT → LED pattern mapping

## Code Guidelines

### General Principles

1. **Minimal changes**: Make surgical, focused modifications. Don't refactor working code unnecessarily.
2. **Embedded constraints**: The MCU has limited memory and no_std environment. Keep allocations minimal.
3. **WASM compatibility**: App code targeting WASM has special considerations (async, no threads, browser APIs).
4. **Preserve functionality**: Don't remove or modify working code unless directly related to your task.

### Rust Style

- Follow standard Rust conventions (rustfmt, clippy)
- Use `no_std` in the MCU and common crates
- Prefer `heapless` collections in embedded code
- Use `postcard` for serialization between MCU and app

### Web Bluetooth Pattern (App)

The app uses a message-passing pattern to avoid RefCell panics:
- Async Bluetooth operations push messages to a queue
- UI processes messages each frame
- Always call `ctx.request_repaint()` after pushing messages from async tasks
- See `app/spec.md` for detailed behavioral contract

### Testing

**App:**
```bash
cd app
cargo check --no-default-features --features font_hack
cargo fmt --check
cargo clippy
# Manual browser testing with DevTools for WASM features
```

**MCU:**
```bash
cd mcu
cargo check --all-targets
cargo fmt --check
cargo clippy
```

**Common:**
```bash
cd common
cargo check
cargo test
```

## Key Components

### Configuration System

- `common/src/config.rs`: Shared configuration types (AppConfig, ChannelConfig, NeopixelMatrixPattern)
- `CONFIG_VERSION`: Version constant for config schema
- Serialized with `postcard` for compact binary representation
- Transferred via Bluetooth GATT characteristic

### Bluetooth Communication

- **MCU side**: GATT server with characteristic for config read/write
- **App side**: Web Bluetooth API wrapper in `app/src/web_bluetooth.rs`
- Connection states: Disconnected → Connecting → Connected → Broken (on error)
- Heartbeat mechanism keeps GATT connection alive

### LED Patterns

- `Stripes`: 4-channel vertical stripes
- `Bars`: 8-channel horizontal bars  
- `Quarters`: 4-channel quadrant layout
- Each channel maps FFT frequency range to RGB LED output

## Important Files

- `app/src/app.rs`: Main UI logic and state management
- `app/src/web_bluetooth.rs`: Bluetooth abstraction layer
- `app/spec.md`: Detailed spec for Web Bluetooth refactor
- `mcu/src/main.rs`: MCU entry point and task setup
- `mcu/src/bluetooth.rs`: GATT server implementation
- `mcu/src/lights.rs`: LED control and pattern rendering
- `common/src/config.rs`: Shared configuration types

## CI/CD

The PR check workflow (`.github/workflows/pr-check.yml`) runs:
- `cargo check` on both app (with WASM target) and mcu (with ESP target)
- Separate jobs for each workspace with appropriate toolchains

## Common Issues and Solutions

1. **Borrow conflicts in app**: Use message-passing pattern, not direct mutation from async tasks
2. **MCU memory constraints**: Use static allocation where possible, `heapless` collections
3. **WASM async**: Use `wasm-bindgen-futures` and `gloo-timers` for async operations
4. **Bluetooth disconnects**: Check heartbeat implementation and error handling in Broken state

## Additional Resources

- App build documentation: `app/README.md`
- MCU notes: `mcu/README.md`
- Web Bluetooth refactor spec: `app/spec.md`
- TODO list: `TODO.md`
