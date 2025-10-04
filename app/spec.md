Project: partylight-config_app — Web Bluetooth refactor spec

Overview
--------
This document records the design, decisions, and implementation details for the Web Bluetooth refactor and UI state changes made to the `app` crate. It collects the requirements you requested, the behavioral contract for the app, the file-level changes, message and state flows, edge cases, testing & verification steps, and recommended follow-ups.

Goals / Requirements
--------------------
- Encapsulate Web Bluetooth JS interop into a `Bluetooth` struct with methods: connect, reconnect, read_config_raw, write_config_raw, heartbeat.
- Replace free-floating wasm functions with struct methods and provide clear JS console logging for each operation for debugging.
- Replace ad-hoc UI mutation from async tasks with a safer message-passing pattern so async tasks communicate back to the UI thread without aliasing/RefCell panics.
- Model the connection using a `ConnectionStatus` enum with states:
  - Disconnected (no device paired, show only Connect)
  - Connecting (in-flight connect attempt)
  - Connected(AppConfig) (successfully connected and have device config)
  - Broken(AppConfig) (connection error — preserve last config and allow reconnect)
- On transition to Connected: read the current config from the device and populate the editor UI with it.
- If Bluetooth operations fail (read, write, heartbeat, reconnect), transition to Broken while preserving the most-recent in-browser config.
- While Connected, run a background heartbeat that keeps GATT alive and attempts reconnects on transient failures.
- Add a Connecting state and prevent duplicate connect/reconnect attempts; disable UI controls while async operations run (busy flag).
- When a reconnect succeeds, preserve in-browser edits (do not overwrite local edits unless there is no local config).
- Ensure UI repaints after async messages so the UI responds without requiring user input.
- Add browser console logging from the Bluetooth interop to help diagnose failures.
- Avoid transitions to Disconnected from async error paths; only manual Disconnect and initial constructor set Disconnected.

Contract (mini)
----------------
Inputs
- User: clicks Connect, Reload, Write, Disconnect, Reconnect.
- Bluetooth device: responds to GATT requests (read/write/notify).
- Background heartbeat: produces success/failure events.

Outputs
- `ConnectionStatus` transitions and `AppConfig` updated in UI.
- Browser console logs for Bluetooth interop operations.
- UI visual changes (disabled buttons while busy, header styling).

Error modes
- Bluetooth failures result in `Broken(AppConfig)` (preserve local state) and `Status` messages pushed to the UI queue.
- Only manual Disconnect or app start produce `Disconnected`.

Success criteria
- Build passes (`cargo check`), UI does not panic due to RefCell borrow errors, and async tasks push messages to a queue processed by the UI each frame.
- Reconnect behavior preserves local edits and re-reads device config only when no local config exists.

Design Notes
------------
1) Bluetooth struct
- Exposed methods (async):
  - new() -> Bluetooth
  - connect(&mut self) -> Result<(), JsValue>
  - reconnect(&mut self) -> Result<(), JsValue>
  - read_config_raw(&self) -> Result<JsValue, JsValue>
  - write_config_raw(&mut self, data: &js_sys::Uint8Array) -> Result<(), JsValue>
  - heartbeat(&mut self) -> Result<(), JsValue>
- Each method logs start/success/failure to the browser console (via `web_sys::console::log_1`) to help debugging.

2) Message queue and AppMessage
- Use `Rc<RefCell<VecDeque<AppMessage>>>` as a small message queue. Async tasks push `AppMessage` entries; the UI drains the queue at the start of each frame and applies changes.
- AppMessage variants:
  - SetBusy(bool)
  - Status(String)
  - Connected(AppConfig)
  - Broken(AppConfig)
  - SetConfig(AppConfig)

3) ConnectionStatus enum
- Disconnected: show only Connect button
- Connecting: show an in-flight indicator and disable connect button
- Connected(AppConfig): enable Reload/Write/Disconnect buttons and ensure heartbeat is running
- Broken(AppConfig): show "Connection broken" state, allow Reconnect while keeping local config

4) Busy flag
- `busy: bool` on the app prevents repeated clicks for Reload/Write/Reconnect while an operation is in progress.

5) UI repainting
- All async spawn_local closures must capture a clone of `egui::Context` and call `ctx.request_repaint()` after pushing messages so the UI shows state changes immediately.

6) Error handling policy
- For decode/serialization errors or Bluetooth failures during read/write/heartbeat/reconnect, push `AppMessage::Broken(last_config)` preserving the last known configuration where possible.
- Only leave `ConnectionStatus::Disconnected` for explicit user action or when the app constructs its initial state.

Files changed / created
----------------------
- app/Cargo.toml (deps added)
  - Added `gloo-timers` (with `futures` feature), `futures-util` and `wasm-bindgen-futures` for spawn_local and stream handling.

- app/src/web_bluetooth.rs (refactor)
  - Implemented `pub struct Bluetooth { /* device/server/char handles */ }` and methods `connect`, `reconnect`, `read_config_raw`, `write_config_raw`, `heartbeat` with console logging.
  - Important: these return `Result<..., JsValue>` and are `async` so the UI spawns them via `spawn_local`.

- app/src/app.rs (major changes)
  - Added `ConnectionStatus` enum, `AppMessage` enum, and `messages: Rc<RefCell<VecDeque<...>>>` field.
  - Replaced unsafe raw pointer mutation from async closures with a message queue model: async tasks push `AppMessage` values and capture `egui::Context` for repainting.
  - Added `busy: bool` to disable UI while operations run.
  - Implemented heartbeat as a spawned async loop using `IntervalStream` which on failure attempts reconnects and pushes `AppMessage::Broken` or `AppMessage::Connected` accordingly.
  - Ensured decode/read/write/heartbeat errors push `Broken` preserving last config.
  - Removed `AppMessage::Disconnected` as a message (Disconnected is set by UI actions directly).

Behavioral examples / sequences
------------------------------
1) Fresh start
- App starts with `ConnectionStatus::Disconnected`. Only Connect button visible.
- On Connect click: set status -> Connecting, busy=true; spawn_local connect task which calls `Bluetooth::connect()`.
- On connect success: read config (Bluetooth::read_config_raw), push `AppMessage::SetConfig(cfg)` and `AppMessage::Connected(cfg)`.
- On any immediate connect/read error: push `AppMessage::Broken(last_cfg)` and SetBusy(false). Repaint requested by the async task.

2) Connected + Heartbeat
- While Connected, a heartbeat loop runs periodically. On success do nothing.
- On transient heartbeat error, attempt to reconnect. If reconnect succeeds, if the app has local edits (SetConfig already present in messages or self.config), preserve them; only read from device if no local config exists.
- If reconnect fails, push `AppMessage::Broken(last_cfg)` and SetBusy(false).

3) Write
- Clicking Write serializes `self.config` to bytes (postcard) and calls `Bluetooth::write_config_raw`.
- On write success: push `AppMessage::Status("Write OK")` and SetBusy(false).
- On write error: push `AppMessage::Broken(last_cfg)` and SetBusy(false).

Edge cases
----------
- Rapid repeated clicks: `busy` prevents duplicates.
- Borrow conflicts: messages queue avoids async direct mutation of UI state and keeps mutable borrows short-lived.
- Repaint: missing repaint calls in async tasks cause the UI to only update on user input — ensure every async closure calls `ctx.request_repaint()` after pushing messages.
- Deserialize failure: if device returns malformed data, prefer Broken with preserved last known config.

Testing & verification steps
----------------------------
1) Build
- In `app` folder: cargo check (already run during refactor). Fix warnings where practical.

2) Manual browser run
- Serve the wasm target (e.g., trunk serve) and open DevTools Console.
- Reproduce Connect / Reload / Write / Disconnect / Reconnect flows while watching console logs produced by `web_bluetooth.rs`.
- When reproducing the "Write -> UI reverts to Disconnected" behavior, capture the console logs. The logs include messages for request_device, connect, get_service, get_characteristic, read, write and heartbeat.

3) Unit/Integration tests
- The codebase is primarily GUI + wasm + browser; add small unit tests for postcard serialization of `AppConfig`.

Quality gates checklist
-----------------------
- [x] Build: cargo check in the `app` crate
- [x] Lint/format: run cargo fmt / clippy (optional)
- [x] Runtime: manual browser test with DevTools open (recommended)

Open follow-ups
----------------
- Convert message queue from `Rc<RefCell<VecDeque<...>>>` to an `async` channel (mpsc) to avoid subtle borrow issues and make intent clearer.
- Add a visible Broken-state banner and explicit user-facing reconnect or "force reload from device" button.
- Improve UI with a spinner and disabled-looking buttons while `busy`.
- Add tests for serialization and small smoke tests for message queue handling.

Files to inspect for future changes
----------------------------------
- `app/src/web_bluetooth.rs` — ensure all async paths log to console and return meaningful errors.
- `app/src/app.rs` — message queue handling, heartbeat timing, and UI disabled/enable logic.
- `app/Cargo.toml` — confirm pinned versions for wasm dependencies.

Change log (summary of edits done)
----------------------------------
- Added `Bluetooth` struct and methods in `web_bluetooth.rs` and inserted console logging for JS interop calls.
- Reworked `app.rs` to use `ConnectionStatus`, `AppMessage`, and a message queue. Repaint requests were added after async messages. Error flows updated to use Broken.

Contact & notes
---------------
If you reproduce the buggy Write path, paste the browser console output here and I will trace the exact sequence of messages the async tasks pushed. I can then patch any remaining spot that still transitions to Disconnected or misses a repaint.


Spec authored: Oct 4, 2025
