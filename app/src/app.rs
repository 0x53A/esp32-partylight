use common::config::*;
use egui::{self, Button, Color32, FontFamily, FontId, CollapsingHeader};
use ractor_wormhole::ractor::ActorRef;
use ractor_wormhole::ractor::thread_local::ThreadLocalActorSpawner;
use std::sync::{Arc, Mutex};


#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;
#[cfg(target_arch = "wasm32")]
use crate::web_bluetooth::Bluetooth;

use web_time::{Instant, Duration};

// -----------------
// Shared State Types
// -----------------

#[derive(Clone)]
struct AppState {
    config: Option<AppConfig>,
    last_status: String,
    busy: bool,
    conn: ConnectionStatus,
    last_update: Option<Instant>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            config: None,
            last_status: "Idle".to_owned(),
            busy: false,
            conn: ConnectionStatus::Disconnected,
            last_update: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected(AppConfig),
    Broken(AppConfig),
}

// -----------------
// Handler Messages
// -----------------

#[derive(Debug)]
enum HandlerMessage {
    Connect,
    Disconnect,
    Reconnect,
    Reload,
    Write(AppConfig),
    SetBusy(bool),
    SetStatus(String),
    SetConnected(AppConfig),
    SetBroken(AppConfig),
    SetConfig(AppConfig),
    Heartbeat,
    StopHeartbeat,
}

// -----------------
// Handler Implementation
// -----------------

#[cfg(target_arch = "wasm32")]
fn create_handler(state: Arc<Mutex<AppState>>) -> Result<ActorRef<HandlerMessage>, ractor_wormhole::ractor::RactorErr<()>> {
    use ractor_wormhole::util::ThreadLocalFnActor;

    let spawner = ThreadLocalActorSpawner::new();
    
    // Create Bluetooth instance on the heap so we can use it across async boundaries
    let bt = Box::leak(Box::new(Bluetooth::new()));
    let bt_ptr: *mut Bluetooth = bt as *mut _;
    
    let (handler, _) = ThreadLocalFnActor::start_fn_instant(spawner, move |mut ctx| async move {
        let mut heartbeat_running = false;
        
        use ractor_wormhole::deps::futures::StreamExt;
        
        while let Some(msg) = ctx.rx.next().await {
            match msg {
                HandlerMessage::SetBusy(busy) => {
                    let mut state = state.lock().unwrap();
                    state.busy = busy;
                    state.last_update = Some(Instant::now());
                }
                
                HandlerMessage::SetStatus(status) => {
                    let mut state = state.lock().unwrap();
                    state.last_status = status;
                    state.last_update = Some(Instant::now());
                }
                
                HandlerMessage::SetConfig(cfg) => {
                    let mut state = state.lock().unwrap();
                    state.config = Some(cfg);
                    state.last_update = Some(Instant::now());
                }
                
                HandlerMessage::SetConnected(cfg) => {
                    let mut state = state.lock().unwrap();
                    state.conn = ConnectionStatus::Connected(cfg);
                    state.last_update = Some(Instant::now());
                }
                
                HandlerMessage::SetBroken(cfg) => {
                    let mut state = state.lock().unwrap();
                    state.conn = ConnectionStatus::Broken(cfg);
                    state.last_update = Some(Instant::now());
                }
                
                HandlerMessage::Connect => {
                    {
                        let mut state = state.lock().unwrap();
                        state.conn = ConnectionStatus::Connecting;
                        state.last_status = "Connecting...".to_string();
                        state.busy = true;
                        state.last_update = Some(Instant::now());
                    }
                    
                    let state_clone = state.clone();
                    let self_actor_ref = ctx.actor_ref.clone();
                    spawn_local(async move {
                        let res = unsafe { (&mut *bt_ptr).connect().await };
                        match res {
                            Ok(_) => {
                                match unsafe { (&*bt_ptr).read_config_raw().await } {
                                    Ok(jsv) => {
                                        let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                        let mut vec = vec![0u8; u8arr.length() as usize];
                                        u8arr.copy_to(&mut vec[..]);
                                        
                                        if let Ok(cfg) = postcard::from_bytes::<AppConfig>(&vec) {
                                            let mut state = state_clone.lock().unwrap();
                                            state.config = Some(cfg.clone());
                                            state.last_status = "Connected".to_string();
                                            state.conn = ConnectionStatus::Connected(cfg);
                                            state.busy = false;
                                            state.last_update = Some(Instant::now());
                                            // connected - start heartbeat
                                            let _ = self_actor_ref.send_message(HandlerMessage::Heartbeat);
                                        } else {
                                            let mut state = state_clone.lock().unwrap();
                                            state.last_status = "Decode error".to_string();
                                            state.conn = ConnectionStatus::Broken(AppConfig::default());
                                            state.busy = false;
                                            state.last_update = Some(Instant::now());
                                        }
                                    }
                                    Err(e) => {
                                        let mut state = state_clone.lock().unwrap();
                                        state.last_status = format!("Read error: {:?}", e);
                                        state.conn = ConnectionStatus::Broken(AppConfig::default());
                                        state.busy = false;
                                        state.last_update = Some(Instant::now());
                                    }
                                }
                            }
                            Err(e) => {
                                let mut state = state_clone.lock().unwrap();
                                state.last_status = format!("Connect error: {:?}", e);
                                state.conn = ConnectionStatus::Broken(AppConfig::default());
                                state.busy = false;
                                state.last_update = Some(Instant::now());
                            }
                        }
                    });
                }
                
                HandlerMessage::Disconnect => {
                    heartbeat_running = false;
                    let state_clone = state.clone();
                    spawn_local(async move {
                        let _ = unsafe { (&mut *bt_ptr).disconnect().await };
                        let mut state = state_clone.lock().unwrap();
                        state.conn = ConnectionStatus::Disconnected;
                        state.config = None;
                        state.last_status = "Disconnected".to_string();
                        state.last_update = Some(Instant::now());
                    });
                }
                
                HandlerMessage::Reconnect => {
                    {
                        let mut state = state.lock().unwrap();
                        state.busy = true;
                        state.last_status = "Reconnecting...".to_string();
                        state.last_update = Some(Instant::now());
                    }
                    
                    let state_clone = state.clone();
                    spawn_local(async move {
                        let res = unsafe { (&mut *bt_ptr).reconnect().await };
                        match res {
                            Ok(_) => {
                                let has_cfg = {
                                    let state = state_clone.lock().unwrap();
                                    state.config.is_some()
                                };
                                
                                if !has_cfg {
                                    match unsafe { (&*bt_ptr).read_config_raw().await } {
                                        Ok(jsv) => {
                                            let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                            let mut vec = vec![0u8; u8arr.length() as usize];
                                            u8arr.copy_to(&mut vec[..]);
                                            
                                            if let Ok(cfg) = postcard::from_bytes::<AppConfig>(&vec) {
                                                let mut state = state_clone.lock().unwrap();
                                                state.config = Some(cfg.clone());
                                                state.last_status = "Connected".to_string();
                                                state.conn = ConnectionStatus::Connected(cfg);
                                                state.busy = false;
                                                state.last_update = Some(Instant::now());
                                            }
                                        }
                                        Err(e) => {
                                            let mut state = state_clone.lock().unwrap();
                                            state.last_status = format!("Read error: {:?}", e);
                                            let cfg = state.config.clone().unwrap_or_default();
                                            state.conn = ConnectionStatus::Broken(cfg);
                                            state.busy = false;
                                            state.last_update = Some(Instant::now());
                                        }
                                    }
                                } else {
                                    let mut state = state_clone.lock().unwrap();
                                    let cfg = state.config.clone().unwrap();
                                    state.last_status = "Connected".to_string();
                                    state.conn = ConnectionStatus::Connected(cfg);
                                    state.busy = false;
                                    state.last_update = Some(Instant::now());
                                }
                            }
                            Err(e) => {
                                let mut state = state_clone.lock().unwrap();
                                state.last_status = format!("Reconnect error: {:?}", e);
                                let cfg = state.config.clone().unwrap_or_default();
                                state.conn = ConnectionStatus::Broken(cfg);
                                state.busy = false;
                                state.last_update = Some(Instant::now());
                            }
                        }
                    });
                }
                
                HandlerMessage::Reload => {
                    {
                        let mut state = state.lock().unwrap();
                        state.busy = true;
                        state.last_status = "Reloading...".to_string();
                        state.last_update = Some(Instant::now());
                    }
                    
                    let state_clone = state.clone();
                    spawn_local(async move {
                        match unsafe { (&*bt_ptr).read_config_raw().await } {
                            Ok(jsv) => {
                                let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                let mut vec = vec![0u8; u8arr.length() as usize];
                                u8arr.copy_to(&mut vec[..]);
                                
                                match postcard::from_bytes::<AppConfig>(&vec) {
                                    Ok(cfg) => {
                                        let mut state = state_clone.lock().unwrap();
                                        state.config = Some(cfg);
                                        state.last_status = "Reload OK".to_string();
                                        state.busy = false;
                                        state.last_update = Some(Instant::now());
                                    }
                                    Err(e) => {
                                        let mut state = state_clone.lock().unwrap();
                                        state.last_status = format!("Decode error: {:?}", e);
                                        let cfg = state.config.clone().unwrap_or_default();
                                        state.conn = ConnectionStatus::Broken(cfg);
                                        state.busy = false;
                                        state.last_update = Some(Instant::now());
                                    }
                                }
                            }
                            Err(e) => {
                                let mut state = state_clone.lock().unwrap();
                                state.last_status = format!("Reload error: {:?}", e);
                                let cfg = state.config.clone().unwrap_or_default();
                                state.conn = ConnectionStatus::Broken(cfg);
                                state.busy = false;
                                state.last_update = Some(Instant::now());
                            }
                        }
                    });
                }
                
                HandlerMessage::Write(cfg) => {
                    {
                        let mut state = state.lock().unwrap();
                        state.busy = true;
                        state.last_status = "Writing...".to_string();
                        state.last_update = Some(Instant::now());
                    }
                    
                    let state_clone = state.clone();
                    if let Ok(bytes) = cfg.to_bytes::<1024>() {
                        spawn_local(async move {
                            let u8arr = js_sys::Uint8Array::from(&bytes[..]);
                            let res = unsafe { (&*bt_ptr).write_config_raw(&u8arr).await };
                            
                            match res {
                                Ok(_) => {
                                    let mut state = state_clone.lock().unwrap();
                                    state.last_status = "Write OK".to_string();
                                    state.busy = false;
                                    state.last_update = Some(Instant::now());
                                }
                                Err(e) => {
                                    let mut state = state_clone.lock().unwrap();
                                    state.last_status = format!("Write error: {:?}", e);
                                    let cfg = state.config.clone().unwrap_or_default();
                                    state.conn = ConnectionStatus::Broken(cfg);
                                    state.busy = false;
                                    state.last_update = Some(Instant::now());
                                }
                            }
                        });
                    } else {
                        let mut state = state_clone.lock().unwrap();
                        state.last_status = "Serialize error".to_string();
                        state.busy = false;
                        state.last_update = Some(Instant::now());
                    }
                }
                
                HandlerMessage::Heartbeat => {
                    if !heartbeat_running {
                        heartbeat_running = true;
                        let state_clone = state.clone();
                        
                        spawn_local(async move {
                            let mut interval = gloo_timers::future::IntervalStream::new(5000);
                            
                            while (interval.next().await).is_some() {
                                let should_continue = {
                                    let state = state_clone.lock().unwrap();
                                    matches!(state.conn, ConnectionStatus::Connected(_))
                                };
                                
                                if !should_continue {
                                    break;
                                }
                                
                                let hb_res = unsafe { (&*bt_ptr).heartbeat().await };
                                if let Err(_e) = hb_res {
                                    // Attempt reconnect
                                    let mut reconnected = false;
                                    for _attempt in 0..3 {
                                        gloo_timers::future::sleep(Duration::from_millis(1000)).await;
                                        if unsafe { (&mut *bt_ptr).reconnect().await }.is_ok() {
                                            reconnected = true;
                                            let mut state = state_clone.lock().unwrap();
                                            state.last_status = "Reconnected".to_string();
                                            state.last_update = Some(Instant::now());
                                            break;
                                        }
                                    }
                                    
                                    if !reconnected {
                                        let mut state = state_clone.lock().unwrap();
                                        state.last_status = "Connection broken".to_string();
                                        let cfg = state.config.clone().unwrap_or_default();
                                        state.conn = ConnectionStatus::Broken(cfg);
                                        state.last_update = Some(Instant::now());
                                        break;
                                    }
                                }
                            }
                        });
                    }
                }
                
                HandlerMessage::StopHeartbeat => {
                    heartbeat_running = false;
                }
            }
        }
    })?;
    
    Ok(handler)
}

#[cfg(not(target_arch = "wasm32"))]
fn create_handler(_state: Arc<Mutex<AppState>>) -> Result<ActorRef<HandlerMessage>, ractor_wormhole::ractor::RactorErr<()>> {
    use ractor_wormhole::util::ThreadLocalFnActor;
    
    let spawner = ThreadLocalActorSpawner::new();
    let (handler, _) = ThreadLocalFnActor::start_fn_instant(spawner, move |mut ctx| async move {
        use ractor_wormhole::deps::futures::StreamExt;
        
        while let Some(_msg) = ctx.rx.next().await {
            // No-op for non-WASM
        }
    })?;
    
    Ok(handler)
}

// -----------------
// Main App Structure
// -----------------

pub struct PartylightApp {
    state: Arc<Mutex<AppState>>,
    handler: ActorRef<HandlerMessage>,
    styled: bool,
}

impl Default for PartylightApp {
    fn default() -> Self {
        let state = Arc::new(Mutex::new(AppState::default()));
        let handler = create_handler(state.clone()).expect("Failed to create handler");
        
        Self {
            state,
            handler,
            styled: false,
        }
    }
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
}

#[cfg(target_arch = "wasm32")]
impl PartylightApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        // Apply styling once
        if !self.styled {
            self.apply_theme(ctx);
            self.styled = true;
        }
        
        let state = self.state.clone();
        let mut state = state.lock().unwrap();
        
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_header(ui);
            ui.add_space(64.0);
            
            // Connection controls
            self.draw_connection_controls(ui, &mut state);
            
            // Config editor (only when config is loaded)
            if state.config.is_some() {
                ui.separator();
                self.draw_config_editor(ui, &mut state);
            }
        });
        
        // Request repaint for animations/updates
        ctx.request_repaint_after(Duration::from_secs(1));
    }
    
    fn apply_theme(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        
        // Pitch-black background
        style.visuals.extreme_bg_color = colors::BLACK;
        style.visuals.window_fill = colors::BLACK;
        style.visuals.panel_fill = colors::BLACK;
        
        // Text color
        style.visuals.override_text_color = Some(colors::YELLOW);
        
        // Button styling
        let stroke = colors::border_stroke();
        style.visuals.widgets.inactive.bg_fill = colors::BLACK;
        style.visuals.widgets.inactive.fg_stroke = stroke;
        style.visuals.widgets.inactive.expansion = 2.0;
        
        style.visuals.widgets.hovered.bg_fill = colors::PINK;
        style.visuals.widgets.hovered.fg_stroke = stroke;
        
        style.visuals.widgets.active.bg_fill = colors::ACTIVE_PINK;
        style.visuals.widgets.active.fg_stroke = stroke;
        
        ctx.set_style(style);
    }
    
    fn draw_header(&self, ui: &mut egui::Ui) {
        let painter = ui.painter();
        let rect = ui.max_rect();
        let x = rect.left() + 24.0;
        let y = rect.top() + 18.0;
        let text = "Diskomator 9000 Pro Max Config Editor";
        
        // Pink shadow behind
        painter.text(
            egui::pos2(x + 4.0, y + 2.0),
            egui::Align2::LEFT_TOP,
            text,
            FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
            Color32::from_rgb(255, 45, 149),
        );
        
        // Foreground yellow
        painter.text(
            egui::pos2(x, y),
            egui::Align2::LEFT_TOP,
            text,
            FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
            Color32::from_rgb(255, 212, 0),
        );
    }
    
    fn draw_connection_controls(&mut self, ui: &mut egui::Ui, state: &AppState) {
        match &state.conn {
            ConnectionStatus::Disconnected => {
                ui.horizontal(|ui| {
                    if ui.add(Button::new("Connect")).clicked() {
                        let _ = self.handler.send_message(HandlerMessage::Connect);
                    }
                });
            }
            
            ConnectionStatus::Connecting => {
                ui.horizontal(|ui| {
                    ui.label("Connecting...");
                    ui.add_enabled(false, Button::new("Connect"));
                });
            }
            
            ConnectionStatus::Connected(_cfg) => {
                ui.horizontal(|ui| {
                    ui.label("Connected");
                    
                    if ui.add_enabled(!state.busy, Button::new("Reload")).clicked() {
                        let _ = self.handler.send_message(HandlerMessage::Reload);
                    }
                    
                    if ui.add_enabled(!state.busy, Button::new("Write")).clicked() {
                        if let Some(cfg) = &state.config {
                            let _ = self.handler.send_message(HandlerMessage::Write(cfg.clone()));
                        }
                    }
                    
                    if ui.add_enabled(!state.busy, Button::new("Disconnect")).clicked() {
                        let _ = self.handler.send_message(HandlerMessage::StopHeartbeat);
                        let _ = self.handler.send_message(HandlerMessage::Disconnect);
                    }
                });
            }
            
            ConnectionStatus::Broken(_cfg) => {
                ui.horizontal(|ui| {
                    ui.label("Connection broken");
                    
                    if ui.add_enabled(!state.busy, Button::new("Reconnect")).clicked() {
                        let _ = self.handler.send_message(HandlerMessage::Reconnect);
                    }
                });
            }
        }
        
        // Status display
        ui.horizontal(|ui| {
            ui.label(format!("Status: {}", state.last_status));
            
            if let Some(last_update) = state.last_update {
                let elapsed = last_update.elapsed().as_secs_f32();
                let color = if elapsed < 1.0 {
                    Color32::GREEN
                } else if elapsed < 5.0 {
                    Color32::YELLOW
                } else {
                    Color32::RED
                };
                ui.colored_label(color, format!("({:.1}s ago)", elapsed));
            }
        });
    }
    
    fn draw_config_editor(&self, ui: &mut egui::Ui, state: &mut AppState) {
        
        // only render the editor when we have a config loaded from the device
        if let Some(cfg) = &mut state.config {
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
        }
        
        // Preset buttons
        ui.label("Load preset:");
        ui.horizontal(|ui| {
            if ui.button("Stripes").clicked() {
                let _ = self.handler.send_message(HandlerMessage::SetConfig(AppConfig::stripes()));
                let _ = self.handler.send_message(HandlerMessage::SetStatus("Loaded Stripes preset".to_string()));
            }
            if ui.button("Bars").clicked() {
                let _ = self.handler.send_message(HandlerMessage::SetConfig(AppConfig::bars()));
                let _ = self.handler.send_message(HandlerMessage::SetStatus("Loaded Bars preset".to_string()));
            }
            if ui.button("Bars2").clicked() {
                let _ = self.handler.send_message(HandlerMessage::SetConfig(AppConfig::bars2()));
                let _ = self.handler.send_message(HandlerMessage::SetStatus("Loaded Bars2 preset".to_string()));
            }
            if ui.button("Quarters").clicked() {
                let _ = self.handler.send_message(HandlerMessage::SetConfig(AppConfig::quarters()));
                let _ = self.handler.send_message(HandlerMessage::SetStatus("Loaded Quarters preset".to_string()));
            }
        });
        
        ui.separator();
        
        // Re-acquire state for pattern editing
        if let Some(cfg) = &mut state.config {
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
            convert_pattern_if_needed(cfg, pattern_idx);
            
            // Render editor for active pattern
            self.draw_pattern_editor(ui, cfg);
        }
    }
    
    fn draw_pattern_editor(&self, ui: &mut egui::Ui, cfg: &mut AppConfig) {
        match &mut cfg.pattern {
            NeopixelMatrixPattern::Stripes(chs) => {
                ui.label("Stripes (4 channels)");
                for (i, ch) in chs.iter_mut().enumerate() {
                    self.draw_channel_editor(ui, i, ch, "Channel");
                }
            }
            NeopixelMatrixPattern::Bars(chs) => {
                ui.label("Bars (8 channels)");
                for (i, ch) in chs.iter_mut().enumerate() {
                    self.draw_channel_editor(ui, i, ch, "Bar");
                }
            }
            NeopixelMatrixPattern::Quarters(chs) => {
                ui.label("Quarters (4 channels)");
                for (i, ch) in chs.iter_mut().enumerate() {
                    self.draw_channel_editor(ui, i, ch, "Quarter");
                }
            }
        }
    }
    
    fn draw_channel_editor(&self, ui: &mut egui::Ui, index: usize, ch: &mut ChannelConfig, label: &str) {
        CollapsingHeader::new(format!("{} {}", label, index)).default_open(true).show(ui, |ui| {
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

// Provide a native (non-wasm) UI stub so the app can still run natively.
#[cfg(not(target_arch = "wasm32"))]
impl PartylightApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        let state = self.state.clone();
        let mut state = state.lock().unwrap();
        
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
            
            if let Some(cfg) = &mut state.config {
                ui.label("Basic settings:");
                ui.horizontal(|ui| {
                    ui.label("Sample count:");
                    let mut sc = cfg.sample_count as u32;
                    if ui.add(egui::widgets::DragValue::new(&mut sc)).changed() {
                        cfg.sample_count = sc as usize;
                    }
                });
            }
        });
    }
}

// Helpers


    
    fn convert_pattern_if_needed(cfg: &mut AppConfig, selected_idx: usize) {
        match (selected_idx, &mut cfg.pattern) {
            (0, NeopixelMatrixPattern::Stripes(_)) => {}
            (0, other) => {
                let new = convert_to_stripes(other);
                cfg.pattern = NeopixelMatrixPattern::Stripes(new);
            }
            (1, NeopixelMatrixPattern::Bars(_)) => {}
            (1, other) => {
                let new = convert_to_bars(other);
                cfg.pattern = NeopixelMatrixPattern::Bars(new);
            }
            (2, NeopixelMatrixPattern::Quarters(_)) => {}
            (2, other) => {
                let new = convert_to_quarters(other);
                cfg.pattern = NeopixelMatrixPattern::Quarters(new);
            }
            _ => {}
        }
    }
    
    fn convert_to_stripes(pattern: &NeopixelMatrixPattern) -> [ChannelConfig; 4] {
        let mut new = std::array::from_fn(|_| ChannelConfig {
            start_index: 0,
            end_index: 0,
            premult: 1.0,
            noise_gate: 0.0,
            exponent: 1,
            color: [1.0, 1.0, 1.0],
            aggregate: AggregationMethod::Sum,
        });
        match pattern {
            NeopixelMatrixPattern::Stripes(chs) | NeopixelMatrixPattern::Quarters(chs) => {
                for i in 0..4 {
                    new[i] = chs[i].clone();
                }
            }
            NeopixelMatrixPattern::Bars(chs) => {
                for i in 0..4 {
                    new[i] = chs[i].clone();
                }
            }
        }
        new
    }
    
    fn convert_to_bars(pattern: &NeopixelMatrixPattern) -> [ChannelConfig; 8] {
        let mut new = std::array::from_fn(|_| ChannelConfig {
            start_index: 0,
            end_index: 0,
            premult: 1.0,
            noise_gate: 0.0,
            exponent: 1,
            color: [1.0, 1.0, 1.0],
            aggregate: AggregationMethod::Sum,
        });
        match pattern {
            NeopixelMatrixPattern::Stripes(chs) | NeopixelMatrixPattern::Quarters(chs) => {
                for i in 0..4 {
                    new[i] = chs[i].clone();
                }
            }
            NeopixelMatrixPattern::Bars(chs) => {
                for i in 0..8 {
                    new[i] = chs[i].clone();
                }
            }
        }
        new
    }
    
    fn convert_to_quarters(pattern: &NeopixelMatrixPattern) -> [ChannelConfig; 4] {
        let mut new = std::array::from_fn(|_| ChannelConfig {
            start_index: 0,
            end_index: 0,
            premult: 1.0,
            noise_gate: 0.0,
            exponent: 1,
            color: [1.0, 1.0, 1.0],
            aggregate: AggregationMethod::Sum,
        });
        match pattern {
            NeopixelMatrixPattern::Stripes(chs) | NeopixelMatrixPattern::Quarters(chs) => {
                for i in 0..4 {
                    new[i] = chs[i].clone();
                }
            }
            NeopixelMatrixPattern::Bars(chs) => {
                for i in 0..4 {
                    new[i] = chs[i].clone();
                }
            }
        }
        new
    }