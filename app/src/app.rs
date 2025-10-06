use common::config::*;
use egui::{self, Button, Color32, FontFamily, FontId};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use js_sys;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

// Import JS helper functions as returning Promises; we'll await them via JsFuture.
#[cfg(target_arch = "wasm32")]
use crate::web_bluetooth::Bluetooth;
#[cfg(target_arch = "wasm32")]
use futures_util::StreamExt;
#[cfg(target_arch = "wasm32")]
use gloo_timers::future::IntervalStream;

pub struct PartylightApp {
    config: Option<AppConfig>,
    last_status: String,
    styled: bool,
    // when true, long-running operations like reload/write are in progress
    busy: bool,
    #[cfg(target_arch = "wasm32")]
    // message queue for async tasks to communicate back to the UI thread
    messages: Rc<RefCell<VecDeque<AppMessage>>>,
    #[cfg(target_arch = "wasm32")]
    bt: Bluetooth,
    #[cfg(target_arch = "wasm32")]
    // heartbeat cancellation token: set to true to allow heartbeat, false to stop
    hb_run: Rc<RefCell<bool>>,
    #[cfg(target_arch = "wasm32")]
    conn: ConnectionStatus,
}

impl Default for PartylightApp {
    fn default() -> Self {
        Self {
            config: None,
            last_status: "Idle".to_owned(),
            styled: false,
            busy: false,
            #[cfg(target_arch = "wasm32")]
            messages: Rc::new(RefCell::new(VecDeque::new())),
            #[cfg(target_arch = "wasm32")]
            bt: Bluetooth::new(),
            #[cfg(target_arch = "wasm32")]
            hb_run: Rc::new(RefCell::new(false)),
            #[cfg(target_arch = "wasm32")]
            conn: ConnectionStatus::Disconnected,
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug)]
pub enum AppMessage {
    SetBusy(bool),
    Status(String),
    Connected(AppConfig),
    Broken(AppConfig),
    SetConfig(AppConfig),
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected(AppConfig),
    Broken(AppConfig),
}

pub mod colors {
    use egui::{Color32, Stroke};

    /// Accent yellow used for titles, text and borders
    pub const YELLOW: Color32 = Color32::from_rgb(255, 212, 0);
    /// Pink accent used for shadow and hover
    pub const PINK: Color32 = Color32::from_rgb(255, 45, 149);
    /// Default black background
    pub const BLACK: Color32 = Color32::from_rgb(0, 0, 0);
    /// Slightly darker pink for active/pressed state
    pub const ACTIVE_PINK: Color32 = Color32::from_rgb(200, 30, 120);

    pub fn yellow_stroke(width: f32) -> Stroke {
        Stroke::new(width, YELLOW)
    }

    pub fn border_stroke() -> Stroke {
        yellow_stroke(2.0)
    }

    pub fn pink_stroke() -> Stroke {
        Stroke::new(2.0, PINK)
    }
}

#[cfg(target_arch = "wasm32")]
impl PartylightApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        // Apply a simple dark + yellow theme once
        if !self.styled {
            let mut style = (*ctx.style()).clone();
            // pitch-black background
            style.visuals.extreme_bg_color = colors::BLACK;
            style.visuals.window_fill = colors::BLACK;
            style.visuals.panel_fill = colors::BLACK;
            // text color
            style.visuals.override_text_color = Some(colors::YELLOW);

            // Button styling: black inner background and pink hover (matching title accent)
            let black = colors::BLACK;
            let pink = colors::PINK;
            let stroke = colors::border_stroke();
            // Inactive button: black background, yellow border
            style.visuals.widgets.inactive.bg_fill = black;
            style.visuals.widgets.inactive.fg_stroke = stroke; // thicker yellow stroke
            // expand so stroke is visible outside the tight button rect
            style.visuals.widgets.inactive.expansion = 2.0;

            // Hover: pink background (title accent), but keep yellow border for consistency
            style.visuals.widgets.hovered.bg_fill = pink;
            style.visuals.widgets.hovered.fg_stroke = stroke;

            // Active (pressed): slightly darker pink to indicate pressed state, keep yellow stroke
            style.visuals.widgets.active.bg_fill = colors::ACTIVE_PINK;
            style.visuals.widgets.active.fg_stroke = stroke;
            // rounding skipped: using default corner radius to remain compatible with current egui

            // style.visuals.button_frame = false;
            // style.visuals.widgets.inactive.bg_stroke = stroke;
            // style.visuals.widgets.hovered.bg_stroke = colors::pink_stroke();

            ctx.set_style(style);
            self.styled = true;
        }

        // Process messages from async tasks
        #[cfg(target_arch = "wasm32")]
        {
            let mut q = self.messages.borrow_mut();
            while let Some(msg) = q.pop_front() {
                match msg {
                    AppMessage::SetBusy(b) => self.busy = b,
                    AppMessage::Status(s) => self.last_status = s,
                    AppMessage::Connected(cfg) => {
                        self.conn = ConnectionStatus::Connected(cfg);
                    }
                    AppMessage::Broken(cfg) => {
                        self.conn = ConnectionStatus::Broken(cfg);
                    }
                    // Note: Disconnected is handled directly via ConnectionStatus in UI actions; async tasks use Broken to preserve config
                    AppMessage::SetConfig(cfg) => {
                        self.config = Some(cfg);
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Decorative header: emulate two-tone misprint by painting text twice with offsets
            let painter = ui.painter();
            let rect = ui.max_rect();
            let x = rect.left() + 24.0; // left-align with a small left margin
            let y = rect.top() + 18.0;
            let text = "Diskomator 9000 Pro Max Config Editor";
            // pink shadow behind
            painter.text(
                egui::pos2(x + 4.0, y + 2.0),
                egui::Align2::LEFT_TOP,
                text,
                FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
                Color32::from_rgb(255, 45, 149),
            );
            // foreground yellow
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_TOP,
                text,
                FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
                Color32::from_rgb(255, 212, 0),
            );

            // add vertical gap so the button appears below the header
            ui.add_space(64.0);

            // When disconnected at boot, only show a single Connect button and nothing else
            #[cfg(target_arch = "wasm32")]
            match &self.conn {
                        ConnectionStatus::Disconnected => {
                            ui.horizontal(|ui| {
                                if ui.add(Button::new("Connect")).clicked() {
                                    // mark as connecting immediately to prevent duplicate clicks
                                    self.conn = ConnectionStatus::Connecting;
                                    self.last_status = "Connecting...".into();
                                    let bt_ptr: *mut Bluetooth = &mut self.bt as *mut _;
                                    let messages = self.messages.clone();
                                    let ctx = ui.ctx().clone();
                                    let hb_run = self.hb_run.clone();
                                    spawn_local(async move {
                                        // ensure busy flag is cleared/updated via messages
                                        messages.borrow_mut().push_back(AppMessage::SetBusy(true));
                                        let res = unsafe { (&mut *bt_ptr).connect().await };
                                        match res {
                                            Ok(_) => {
                                                // read config after connecting
                                                match unsafe { (&*bt_ptr).read_config_raw().await } {
                                                    Ok(jsv) => {
                                                        let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                                        let mut vec = vec![0u8; u8arr.length() as usize];
                                                        u8arr.copy_to(&mut vec[..]);
                                                        if let Ok(cfg) = postcard::from_bytes::<AppConfig>(&vec) {
                                                            messages.borrow_mut().push_back(AppMessage::SetConfig(cfg.clone()));
                                                            messages.borrow_mut().push_back(AppMessage::Status("Connected".into()));
                                                            messages.borrow_mut().push_back(AppMessage::Connected(cfg.clone()));

                                                            // start heartbeat task (only after successful read/decode)
                                                            // mark hb_run = true so spawned heartbeat will continue
                                                            *hb_run.borrow_mut() = true;
                                                            let bt_clone: *mut Bluetooth = bt_ptr as *mut _;
                                                            let messages_hb = messages.clone();
                                                            let hb_ctx = ctx.clone();
                                                            // Spawn the heartbeat loop which will check the hb_run token through shared Rc<RefCell<bool>> captured below.
                                                            let hb_run_token = hb_run.clone();
                                                            spawn_local(async move {
                                                                let mut interval = gloo_timers::future::IntervalStream::new(5000);
                                                                while let Some(_) = interval.next().await {
                                                                    // check cancellation
                                                                    if !*hb_run_token.borrow() {
                                                                        // stopped; break
                                                                        break;
                                                                    }
                                                                    let hb_res = unsafe { (&*bt_clone).heartbeat().await };
                                                                    if let Err(e) = hb_res {
                                                                        // heartbeat failed — attempt reconnect a few times
                                                                        let mut reconnected = false;
                                                                        for _attempt in 0..3 {
                                                                            // small delay before reconnect attempt
                                                                            gloo_timers::future::sleep(std::time::Duration::from_millis(1000)).await;
                                                                            let reconnect_res = unsafe { (&mut *bt_clone).reconnect().await };
                                                                            if reconnect_res.is_ok() {
                                                                                // success: if we don't already have browser edits, refresh from device
                                                                                let has_cfg = {
                                                                                    let q = messages_hb.borrow();
                                                                                    q.iter().any(|m| matches!(m, AppMessage::SetConfig(_)))
                                                                                };
                                                                                if !has_cfg {
                                                                                    if let Ok(jsv) = unsafe { (&*bt_clone).read_config_raw().await } {
                                                                                        let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                                                                        let mut vec = vec![0u8; u8arr.length() as usize];
                                                                                        u8arr.copy_to(&mut vec[..]);
                                                                                        if let Ok(cfg) = postcard::from_bytes::<AppConfig>(&vec) {
                                                                                            messages_hb.borrow_mut().push_back(AppMessage::SetConfig(cfg.clone()));
                                                                                        }
                                                                                    }
                                                                                }
                                                                                messages_hb.borrow_mut().push_back(AppMessage::Status("Reconnected".into()));
                                                                                // keep user edits if present, otherwise use freshly-read config
                                                                                if let Some(cfg_msg) = {
                                                                                    let q = messages_hb.borrow();
                                                                                    q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                                                } {
                                                                                    messages_hb.borrow_mut().push_back(AppMessage::Connected(cfg_msg));
                                                                                }
                                                                                reconnected = true;
                                                                                break;
                                                                            }
                                                                        }
                                                                        if !reconnected {
                                                                            // heartbeat permanently failed: report and mark Broken while preserving last config
                                                                            let last_cfg = {
                                                                                let q = messages_hb.borrow();
                                                                                q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                                            };
                                                                            messages_hb.borrow_mut().push_back(AppMessage::Status(format!("Connection broken: {:?}", e)));
                                                                            if let Some(cfg) = last_cfg {
                                                                                messages_hb.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                                            } else {
                                                                                // transition to Broken, preserving last known config if any
                                                                                let last_cfg = {
                                                                                    let q = messages_hb.borrow();
                                                                                    q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                                                };
                                                                                if let Some(cfg) = last_cfg {
                                                                                    messages_hb.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                                                } else {
                                                                                    messages_hb.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                                                }
                                                                            }
                                                                            hb_ctx.request_repaint();
                                                                            break;
                                                                        }
                                                                    }
                                                                }
                                                            });
                                                        } else {
                                                            messages.borrow_mut().push_back(AppMessage::Status(format!("Decode error: <bad postcard>")));
                                                            // transition to Broken preserving last known config
                                                            let last_cfg = {
                                                                let q = messages.borrow();
                                                                q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                            };
                                                            if let Some(cfg) = last_cfg {
                                                                messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                            } else {
                                                                messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                            }
                                                        }
                                                        ctx.request_repaint();
                                                    }
                                                    Err(e) => {
                                                        messages.borrow_mut().push_back(AppMessage::Status(format!("Read err: {:?}", e)));
                                                        // transition to Broken preserving last known config
                                                        let last_cfg = {
                                                            let q = messages.borrow();
                                                            q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                        };
                                                        if let Some(cfg) = last_cfg {
                                                            messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                        } else {
                                                            messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                        }
                                                        ctx.request_repaint();
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                messages.borrow_mut().push_back(AppMessage::Status(format!("Connect err: {:?}", e)));
                                                // transition to Broken preserving last known config
                                                let last_cfg = {
                                                    let q = messages.borrow();
                                                    q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                };
                                                if let Some(cfg) = last_cfg {
                                                    messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                } else {
                                                    messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                }
                                                ctx.request_repaint();
                                            }
                                        }
                                        messages.borrow_mut().push_back(AppMessage::SetBusy(false));
                                        ctx.request_repaint();
                                    });
                                }
                            });
                            // only show the Connect button when disconnected
                            return;
                        }
                        ConnectionStatus::Connected(_cfg) => {
                            ui.horizontal(|ui| {
                                ui.label("Connected");
                                if ui.add_enabled(!self.busy, Button::new("Reload")).clicked() {
                                    self.last_status = "Reloading...".into();
                                    // mark busy so UI disables other operations
                                    self.busy = true;
                                    let bt_ptr: *mut Bluetooth = &mut self.bt as *mut _;
                                    let messages = self.messages.clone();
                                    let ctx = ui.ctx().clone();
                                    spawn_local(async move {
                                        match unsafe { (&*bt_ptr).read_config_raw().await } {
                                            Ok(jsv) => {
                                                let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                                let mut vec = vec![0u8; u8arr.length() as usize];
                                                u8arr.copy_to(&mut vec[..]);
                                                match postcard::from_bytes::<AppConfig>(&vec) {
                                                    Ok(cfg) => {
                                                        messages.borrow_mut().push_back(AppMessage::SetConfig(cfg.clone()));
                                                        messages.borrow_mut().push_back(AppMessage::Status("Reload OK".into()));
                                                    }
                                                    Err(e) => {
                                                        // decode failed: preserve any existing config in Broken state
                                                        messages.borrow_mut().push_back(AppMessage::Status(format!("Decode error: {:?}", e)));
                                                        // preserve last config if exists
                                                        let last_cfg = {
                                                            let q = messages.borrow();
                                                            q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                        };
                                                        if let Some(cfg) = last_cfg {
                                                            messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                        } else {
                                                            messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                // read failed: mark Broken preserving last config if any
                                                messages.borrow_mut().push_back(AppMessage::Status(format!("Reload err: {:?}", e)));
                                                let last_cfg = {
                                                    let q = messages.borrow();
                                                    q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                };
                                                if let Some(cfg) = last_cfg {
                                                    messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                } else {
                                                    // transition to Broken preserving last known config
                                                    let last_cfg = {
                                                        let q = messages.borrow();
                                                        q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                    };
                                                    if let Some(cfg) = last_cfg {
                                                        messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                    } else {
                                                        messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                    }
                                                }
                                            }
                                            ,
                                        }
                                        messages.borrow_mut().push_back(AppMessage::SetBusy(false));
                                        // request a repaint
                                        ctx.request_repaint();
                                    });
                                }
                                if ui.add_enabled(!self.busy, Button::new("Write")).clicked() {
                                    self.last_status = "Writing...".into();
                                    // mark busy so UI disables other operations
                                    self.busy = true;
                                    let bt_ptr: *mut Bluetooth = &mut self.bt as *mut _;
                                    let messages = self.messages.clone();
                                    // capture current config bytes
                                    let bytes_res = self.config.clone().unwrap().to_bytes::<1024>();
                                    if let Ok(bytes) = bytes_res {
                                        let ctx = ui.ctx().clone();
                                        spawn_local(async move {
                                            let u8arr = js_sys::Uint8Array::from(&bytes[..]);
                                            let res = unsafe { (&*bt_ptr).write_config_raw(&u8arr).await };
                                            match res {
                                                Ok(_) => {
                                                    messages.borrow_mut().push_back(AppMessage::Status("Write OK".into()));
                                                }
                                                Err(e) => {
                                                    // write failed: mark Broken preserving last config if any
                                                    messages.borrow_mut().push_back(AppMessage::Status(format!("Write err: {:?}", e)));
                                                    let last_cfg = messages.borrow().iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None });
                                                    if let Some(cfg) = last_cfg {
                                                        messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                    } else {
                                                        // transition to Broken preserving last known config
                                                        let last_cfg = {
                                                            let q = messages.borrow();
                                                            q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                        };
                                                        if let Some(cfg) = last_cfg {
                                                            messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                        } else {
                                                            messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                        }
                                                    }
                                                }
                                            }
                                                // clear busy and request UI repaint
                                                messages.borrow_mut().push_back(AppMessage::SetBusy(false));
                                                ctx.request_repaint();
                                        });
                                    } else if let Err(e) = bytes_res {
                                        messages.borrow_mut().push_back(AppMessage::Status(format!("Serialize err: {:?}", e)));
                                        messages.borrow_mut().push_back(AppMessage::SetBusy(false));
                                        ui.ctx().request_repaint();
                                    }
                                }
                                if ui.add_enabled(!self.busy, Button::new("Disconnect")).clicked() {
                                    // stop heartbeat loop
                                    *self.hb_run.borrow_mut() = false;
                                    // attempt to gracefully disconnect the bluetooth device
                                    let bt_ptr: *mut Bluetooth = &mut self.bt as *mut _;
                                    spawn_local(async move {
                                        let _ = unsafe { (&mut *bt_ptr).disconnect().await };
                                    });
                                    self.conn = ConnectionStatus::Disconnected;
                                    self.config = None; // clear config from UI
                                    self.last_status = "Disconnected".into();
                                }
                                
                                // OTA Update button
                                if ui.add_enabled(!self.busy, Button::new("OTA Update")).clicked() {
                                    self.last_status = "Select firmware file...".into();
                                    
                                    // Create file input element
                                    use web_sys::{window, HtmlInputElement};
                                    if let Some(window) = window() {
                                        if let Some(document) = window.document() {
                                            if let Ok(input) = document.create_element("input") {
                                                if let Ok(input) = input.dyn_into::<HtmlInputElement>() {
                                                    input.set_type("file");
                                                    input.set_accept(".bin");
                                                    
                                                    let bt_ptr: *mut Bluetooth = &mut self.bt as *mut _;
                                                    let messages = self.messages.clone();
                                                    let ctx = ui.ctx().clone();
                                                    
                                                    // Create change handler
                                                    let onchange = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::Event| {
                                                        use web_sys::FileReader;
                                                        
                                                        if let Some(target) = event.target() {
                                                            if let Ok(input) = target.dyn_into::<HtmlInputElement>() {
                                                                if let Some(files) = input.files() {
                                                                    if files.length() > 0 {
                                                                        if let Some(file) = files.get(0) {
                                                                            let messages_clone = messages.clone();
                                                                            let ctx_clone = ctx.clone();
                                                                            
                                                                            messages_clone.borrow_mut().push_back(AppMessage::SetBusy(true));
                                                                            messages_clone.borrow_mut().push_back(AppMessage::Status(format!("Reading firmware file...")));
                                                                            ctx_clone.request_repaint();
                                                                            
                                                                            // Read file
                                                                            if let Ok(reader) = FileReader::new() {
                                                                                let messages_read = messages_clone.clone();
                                                                                let ctx_read = ctx_clone.clone();
                                                                                let reader_rc = Rc::new(reader);
                                                                                let reader_clone = reader_rc.clone();
                                                                                
                                                                                let onload = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::ProgressEvent| {
                                                                                    // File read complete - perform OTA
                                                                                    if let Ok(result) = reader_clone.result() {
                                                                                        if let Ok(array_buffer) = result.dyn_into::<js_sys::ArrayBuffer>() {
                                                                                            let firmware_data = js_sys::Uint8Array::new(&array_buffer);
                                                                                            let firmware_len = firmware_data.length();
                                                                                            
                                                                                            messages_read.borrow_mut().push_back(AppMessage::Status(format!("Uploading {} bytes...", firmware_len)));
                                                                                            ctx_read.request_repaint();
                                                                                            
                                                                                            let messages_ota = messages_read.clone();
                                                                                            let ctx_ota = ctx_read.clone();
                                                                                            
                                                                                            spawn_local(async move {
                                                                                                // Calculate SHA256 hash using Web Crypto API
                                                                                                let hash_result = async {
                                                                                                    use web_sys::window;
                                                                                                    use wasm_bindgen::JsCast;
                                                                                                    use wasm_bindgen_futures::JsFuture;
                                                                                                    
                                                                                                    let window = window().ok_or("No window")?;
                                                                                                    let crypto = window.crypto().map_err(|_| "No crypto")?;
                                                                                                    let subtle = crypto.subtle();
                                                                                                    
                                                                                                    let promise = subtle.digest_with_str_and_u8_array("SHA-256", &firmware_data.to_vec())
                                                                                                        .map_err(|_| "Digest failed")?;
                                                                                                    let result = JsFuture::from(promise).await.map_err(|_| "Hash calculation failed")?;
                                                                                                    let hash_buffer = result.dyn_into::<js_sys::ArrayBuffer>().map_err(|_| "Invalid hash buffer")?;
                                                                                                    Ok::<js_sys::Uint8Array, &str>(js_sys::Uint8Array::new(&hash_buffer))
                                                                                                }.await;
                                                                                                
                                                                                                match hash_result {
                                                                                                    Ok(hash) => {
                                                                                                        messages_ota.borrow_mut().push_back(AppMessage::Status("Hash calculated, sending to device...".into()));
                                                                                                        ctx_ota.request_repaint();
                                                                                                        
                                                                                                        // Send hash
                                                                                                        if let Err(e) = unsafe { (&*bt_ptr).ota_set_hash(&hash).await } {
                                                                                                            messages_ota.borrow_mut().push_back(AppMessage::Status(format!("Failed to set hash: {:?}", e)));
                                                                                                            messages_ota.borrow_mut().push_back(AppMessage::SetBusy(false));
                                                                                                            ctx_ota.request_repaint();
                                                                                                            return;
                                                                                                        }
                                                                                                        
                                                                                                        // Begin OTA
                                                                                                        if let Err(e) = unsafe { (&*bt_ptr).ota_begin().await } {
                                                                                                            messages_ota.borrow_mut().push_back(AppMessage::Status(format!("Failed to begin OTA: {:?}", e)));
                                                                                                            messages_ota.borrow_mut().push_back(AppMessage::SetBusy(false));
                                                                                                            ctx_ota.request_repaint();
                                                                                                            return;
                                                                                                        }
                                                                                                        
                                                                                                        messages_ota.borrow_mut().push_back(AppMessage::Status("OTA started, sending data...".into()));
                                                                                                        ctx_ota.request_repaint();
                                                                                                        
                                                                                                        // Send firmware in chunks
                                                                                                        let chunk_size = 512;
                                                                                                        let mut offset = 0;
                                                                                                        
                                                                                                        while offset < firmware_len {
                                                                                                            let end = (offset + chunk_size).min(firmware_len);
                                                                                                            let chunk = firmware_data.subarray(offset, end);
                                                                                                            
                                                                                                            if let Err(e) = unsafe { (&*bt_ptr).ota_write_chunk(&chunk).await } {
                                                                                                                messages_ota.borrow_mut().push_back(AppMessage::Status(format!("Failed to write chunk: {:?}", e)));
                                                                                                                let _ = unsafe { (&*bt_ptr).ota_abort().await };
                                                                                                                messages_ota.borrow_mut().push_back(AppMessage::SetBusy(false));
                                                                                                                ctx_ota.request_repaint();
                                                                                                                return;
                                                                                                            }
                                                                                                            
                                                                                                            offset = end;
                                                                                                            
                                                                                                            // Update progress every 10%
                                                                                                            if offset % (firmware_len / 10).max(1) < chunk_size {
                                                                                                                let progress = (offset * 100) / firmware_len;
                                                                                                                messages_ota.borrow_mut().push_back(AppMessage::Status(format!("Uploading... {}%", progress)));
                                                                                                                ctx_ota.request_repaint();
                                                                                                            }
                                                                                                        }
                                                                                                        
                                                                                                        messages_ota.borrow_mut().push_back(AppMessage::Status("Committing update...".into()));
                                                                                                        ctx_ota.request_repaint();
                                                                                                        
                                                                                                        // Commit OTA
                                                                                                        if let Err(e) = unsafe { (&*bt_ptr).ota_commit().await } {
                                                                                                            messages_ota.borrow_mut().push_back(AppMessage::Status(format!("Failed to commit: {:?}", e)));
                                                                                                            messages_ota.borrow_mut().push_back(AppMessage::SetBusy(false));
                                                                                                            ctx_ota.request_repaint();
                                                                                                            return;
                                                                                                        }
                                                                                                        
                                                                                                        messages_ota.borrow_mut().push_back(AppMessage::Status("OTA complete! Device will reboot.".into()));
                                                                                                        messages_ota.borrow_mut().push_back(AppMessage::SetBusy(false));
                                                                                                        ctx_ota.request_repaint();
                                                                                                    }
                                                                                                    Err(e) => {
                                                                                                        messages_ota.borrow_mut().push_back(AppMessage::Status(format!("Hash calculation failed: {}", e)));
                                                                                                        messages_ota.borrow_mut().push_back(AppMessage::SetBusy(false));
                                                                                                        ctx_ota.request_repaint();
                                                                                                    }
                                                                                                }
                                                                                            });
                                                                                        }
                                                                                    }
                                                                                }) as Box<dyn FnMut(_)>);
                                                                                
                                                                                reader_rc.set_onload(Some(onload.as_ref().unchecked_ref()));
                                                                                onload.forget();
                                                                                
                                                                                let _ = reader_rc.read_as_array_buffer(&file);
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }) as Box<dyn FnMut(_)>);
                                                    
                                                    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
                                                    onchange.forget();
                                                    
                                                    input.click();
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                        }
                        ConnectionStatus::Broken(_cfg) => {
                            ui.horizontal(|ui| {
                                ui.label("Connection broken");
                                if ui.add_enabled(!self.busy, Button::new("Reconnect")).clicked() {
                                    self.last_status = "Reconnecting...".into();
                                    // mark busy so other actions are disabled while reconnecting
                                    self.busy = true;
                                    let ctx = ui.ctx().clone();
                                    let bt_ptr: *mut Bluetooth = &mut self.bt as *mut _;
                                    let messages = self.messages.clone();
                                    spawn_local(async move {
                                        let res = unsafe { (&mut *bt_ptr).reconnect().await };
                                        match res {
                                            Ok(_) => {
                                                // preserve browser edits: only reload from device if we don't already have a config
                                                let has_cfg = {
                                                    let q = messages.borrow();
                                                    q.iter().any(|m| matches!(m, AppMessage::SetConfig(_)))
                                                };
                                                if !has_cfg {
                                                    match unsafe { (&*bt_ptr).read_config_raw().await } {
                                                        Ok(jsv) => {
                                                            let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                                            let mut vec = vec![0u8; u8arr.length() as usize];
                                                            u8arr.copy_to(&mut vec[..]);
                                                            if let Ok(cfg) = postcard::from_bytes::<AppConfig>(&vec) {
                                                                messages.borrow_mut().push_back(AppMessage::SetConfig(cfg.clone()));
                                                            }
                                                        }
                                                        Err(e) => {
                                                            // reload/read failed: report and transition to Broken preserving last-known config
                                                            messages.borrow_mut().push_back(AppMessage::Status(format!("Reload err: {:?}", e)));
                                                            let last_cfg = {
                                                                let q = messages.borrow();
                                                                q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                            };
                                                            if let Some(cfg) = last_cfg {
                                                                messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                            } else {
                                                                messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                            }
                                                        },
                                                    }
                                                }
                                                // only transition to Connected if we successfully have a config
                                                if let Some(cfg_msg) = {
                                                    let q = messages.borrow();
                                                    q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                } {
                                                    messages.borrow_mut().push_back(AppMessage::Status("Connected".into()));
                                                    messages.borrow_mut().push_back(AppMessage::Connected(cfg_msg));
                                                } else {
                                                    // reload/read failed earlier; ensure we are Disconnected rather than claiming connected
                                                    messages.borrow_mut().push_back(AppMessage::Status("Reconnect succeeded but no config available".into()));
                                                    // transition to Broken preserving last known config
                                                    let last_cfg = {
                                                        let q = messages.borrow();
                                                        q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                    };
                                                    if let Some(cfg) = last_cfg {
                                                        messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                    } else {
                                                        messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                // reconnect failed: reset to Disconnected so UI isn't stuck
                                                messages.borrow_mut().push_back(AppMessage::Status(format!("Reconnect err: {:?}", e)));
                                                    // transition to Broken preserving last known config
                                                    let last_cfg = {
                                                        let q = messages.borrow();
                                                        q.iter().find_map(|m| match m { AppMessage::SetConfig(c) => Some(c.clone()), _ => None })
                                                    };
                                                    if let Some(cfg) = last_cfg {
                                                        messages.borrow_mut().push_back(AppMessage::Broken(cfg));
                                                    } else {
                                                        messages.borrow_mut().push_back(AppMessage::Broken(AppConfig::default()));
                                                    }
                                            },
                                        }
                                        // clear busy regardless of outcome and request repaint
                                        messages.borrow_mut().push_back(AppMessage::SetBusy(false));
                                        ctx.request_repaint();
                                    });
                                }
                            });
                        }
                        ConnectionStatus::Connecting => {
                            ui.horizontal(|ui| {
                                ui.label("Connecting...");
                                ui.add_enabled(false, Button::new("Connect"));
                            });
                        }
                    }

            // Connection actions are handled above (Connect/Reload/Write/Disconnect/Reconnect)

            // only render the editor when we have a config loaded from the device
            if let Some(cfg) = &mut self.config {
                ui.separator();

                ui.label("Basic settings:");
                ui.horizontal(|ui| {
                    ui.label("Sample count:");
                    let mut sc = cfg.sample_count as u32;
                    if ui.add(egui::widgets::DragValue::new(&mut sc)).changed() {
                        cfg.sample_count = sc as usize;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Use Hann window:");
                    ui.checkbox(&mut cfg.use_hann_window, "");
                });

                ui.separator();

                ui.label("Pattern:");
                // Pattern selector
                let mut pattern_idx = match &cfg.pattern {
                    NeopixelMatrixPattern::Stripes(_) => 0usize,
                    NeopixelMatrixPattern::Bars(_) => 1usize,
                    NeopixelMatrixPattern::Quarters(_) => 2usize,
                };
                egui::ComboBox::from_label("Pattern type")
                    .selected_text(match pattern_idx {
                        0 => "Stripes",
                        1 => "Bars",
                        _ => "Quarters",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut pattern_idx, 0, "Stripes");
                        ui.selectable_value(&mut pattern_idx, 1, "Bars");
                        ui.selectable_value(&mut pattern_idx, 2, "Quarters");
                    });

                // Convert pattern if changed
                let selected_pattern_idx = pattern_idx;
                match (selected_pattern_idx, &mut cfg.pattern) {
                    (0, NeopixelMatrixPattern::Stripes(_)) => {}
                    (0, other) => {
                        let mut new: [ChannelConfig; 4] = std::array::from_fn(|_| ChannelConfig {
                            start_index: 0,
                            end_index: 0,
                            premult: 1.0,
                            noise_gate: 0.0,
                            exponent: 1,
                            color: [1.0, 1.0, 1.0],
                            aggregate: AggregationMethod::Sum,
                        });
                        match other {
                            NeopixelMatrixPattern::Bars(chs) => {
                                for i in 0..4 {
                                    new[i] = chs[i].clone();
                                }
                            }
                            NeopixelMatrixPattern::Quarters(chs) => {
                                for i in 0..4 {
                                    new[i] = chs[i].clone();
                                }
                            }
                            _ => {}
                        }
                        cfg.pattern = NeopixelMatrixPattern::Stripes(new);
                    }
                    (1, NeopixelMatrixPattern::Bars(_)) => {}
                    (1, other) => {
                        let mut new: [ChannelConfig; 8] = std::array::from_fn(|_| ChannelConfig {
                            start_index: 0,
                            end_index: 0,
                            premult: 1.0,
                            noise_gate: 0.0,
                            exponent: 1,
                            color: [1.0, 1.0, 1.0],
                            aggregate: AggregationMethod::Sum,
                        });
                        match other {
                            NeopixelMatrixPattern::Stripes(chs) => {
                                for i in 0..4 {
                                    new[i] = chs[i].clone();
                                }
                            }
                            NeopixelMatrixPattern::Quarters(chs) => {
                                for i in 0..4 {
                                    new[i] = chs[i].clone();
                                }
                            }
                            _ => {}
                        }
                        cfg.pattern = NeopixelMatrixPattern::Bars(new);
                    }
                    (2, NeopixelMatrixPattern::Quarters(_)) => {}
                    (2, other) => {
                        let mut new: [ChannelConfig; 4] = std::array::from_fn(|_| ChannelConfig {
                            start_index: 0,
                            end_index: 0,
                            premult: 1.0,
                            noise_gate: 0.0,
                            exponent: 1,
                            color: [1.0, 1.0, 1.0],
                            aggregate: AggregationMethod::Sum,
                        });
                        match other {
                            NeopixelMatrixPattern::Stripes(chs) => {
                                for i in 0..4 {
                                    new[i] = chs[i].clone();
                                }
                            }
                            NeopixelMatrixPattern::Bars(chs) => {
                                for i in 0..4 {
                                    new[i] = chs[i].clone();
                                }
                            }
                            _ => {}
                        }
                        cfg.pattern = NeopixelMatrixPattern::Quarters(new);
                    }
                    _ => {}
                }

                // Render editor for whichever variant is active
                match &mut cfg.pattern {
                    NeopixelMatrixPattern::Stripes(chs) => {
                        ui.label("Stripes (4 channels)");
                        for (i, ch) in chs.iter_mut().enumerate() {
                            ui.collapsing(format!("Channel {}", i), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("start:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.start_index));
                                    ui.label("end:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.end_index));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("premult:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.premult));
                                    ui.label("noise_gate:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.noise_gate));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("exponent:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.exponent));
                                    ui.label("color (r,g,b):");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[0]).speed(0.01).range(0.0..=1.0));
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[1]).speed(0.01).range(0.0..=1.0));
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[2]).speed(0.01).range(0.0..=1.0));
                                });
                            });
                        }
                    }
                    NeopixelMatrixPattern::Bars(chs) => {
                        ui.label("Bars (8 channels)");
                        for (i, ch) in chs.iter_mut().enumerate() {
                            ui.collapsing(format!("Bar {}", i), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("start:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.start_index));
                                    ui.label("end:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.end_index));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("premult:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.premult));
                                    ui.label("noise_gate:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.noise_gate));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("exponent:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.exponent));
                                    ui.label("color (r,g,b):");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[0]).speed(0.01).range(0.0..=1.0));
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[1]).speed(0.01).range(0.0..=1.0));
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[2]).speed(0.01).range(0.0..=1.0));
                                });
                            });
                        }
                    }
                    NeopixelMatrixPattern::Quarters(chs) => {
                        ui.label("Quarters (4 channels)");
                        for (i, ch) in chs.iter_mut().enumerate() {
                            ui.collapsing(format!("Quarter {}", i), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("start:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.start_index));
                                    ui.label("end:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.end_index));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("premult:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.premult));
                                    ui.label("noise_gate:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.noise_gate));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("exponent:");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.exponent));
                                    ui.label("color (r,g,b):");
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[0]).speed(0.01).range(0.0..=1.0));
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[1]).speed(0.01).range(0.0..=1.0));
                                    ui.add(egui::widgets::DragValue::new(&mut ch.color[2]).speed(0.01).range(0.0..=1.0));
                                });
                            });
                        }
                    }
                }
            }
        });
    }
}

// Provide a native (non-wasm) UI stub so the app can still run natively.
#[cfg(not(target_arch = "wasm32"))]
impl PartylightApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Diskomator 9000 Pro Max Config Editor (WASM only)")
                    .font(egui::FontId::new(
                        22.0,
                        egui::FontFamily::Name(std::sync::Arc::from("Cynatar")),
                    ))
                    .strong(),
            );
            ui.label("Bluetooth functions are only available when compiled to WebAssembly.");

            ui.separator();
            ui.label("Basic settings:");
            ui.horizontal(|ui| {
                ui.label("Sample count:");
                let mut sc = self.config.sample_count as u32;
                if ui.add(egui::widgets::DragValue::new(&mut sc)).changed() {
                    self.config.sample_count = sc as usize;
                }
            });
        });
    }
}
