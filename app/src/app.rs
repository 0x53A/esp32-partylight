use anyhow::Result;
use common::config::*;
use egui::{self, Button, Color32, FontFamily, FontId};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

#[cfg(target_arch = "wasm32")]
use crate::web_bluetooth::Bluetooth;
#[cfg(target_arch = "wasm32")]
use futures_util::StreamExt;

use crate::ractor::ActorRef;

// -----------------
// Colors Module
// -----------------

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

// -----------------
// Shared State Types
// -----------------

#[derive(Clone)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected(AppConfig),
    Broken(AppConfig),
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        ConnectionStatus::Disconnected
    }
}

#[derive(Default, Clone)]
struct AppState {
    config: Option<AppConfig>,
    last_status: String,
    busy: bool,
    conn: ConnectionStatus,
    last_update: Option<Instant>,
}

// -----------------
// Handler Messages
// -----------------

#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
enum HandlerMessage {
    Connect,
    Reload,
    Write,
    Disconnect,
    Reconnect,
    SetBusy(bool),
    SetStatus(String),
    SetConfig(AppConfig),
    SetConnectionStatus(ConnectionStatus),
}

// -----------------
// Handler Implementation
// -----------------

#[cfg(target_arch = "wasm32")]
fn create_handler(
    state: Arc<Mutex<AppState>>,
    bt: Arc<Mutex<Bluetooth>>,
    hb_run: Arc<Mutex<bool>>,
    ctx_clone: egui::Context,
) -> Result<ActorRef<HandlerMessage>> {
    use ractor_wormhole::util::FnActor;

    let (handler, _) = FnActor::start_fn_instant(|mut actor_ctx| async move {
        while let Some(msg) = actor_ctx.rx.recv().await {
            match msg {
                HandlerMessage::SetBusy(b) => {
                    let mut state = state.lock().unwrap();
                    state.busy = b;
                    state.last_update = Some(Instant::now());
                    ctx_clone.request_repaint();
                }
                
                HandlerMessage::SetStatus(s) => {
                    let mut state = state.lock().unwrap();
                    state.last_status = s;
                    state.last_update = Some(Instant::now());
                    ctx_clone.request_repaint();
                }
                
                HandlerMessage::SetConfig(cfg) => {
                    let mut state = state.lock().unwrap();
                    state.config = Some(cfg);
                    state.last_update = Some(Instant::now());
                    ctx_clone.request_repaint();
                }
                
                HandlerMessage::SetConnectionStatus(status) => {
                    let mut state = state.lock().unwrap();
                    state.conn = status;
                    state.last_update = Some(Instant::now());
                    ctx_clone.request_repaint();
                }
                
                HandlerMessage::Connect => {
                    let state_clone = state.clone();
                    let bt_clone = bt.clone();
                    let hb_run_clone = hb_run.clone();
                    let ctx = ctx_clone.clone();
                    let handler_ref = actor_ctx.myself.clone();
                    
                    #[cfg(target_arch = "wasm32")]
                    ractor::concurrency::spawn_local(async move {
                        let _ = handler_ref.send_message(HandlerMessage::SetBusy(true));
                        let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Connecting));
                        
                        let res = {
                            let mut bt_guard = bt_clone.lock().unwrap();
                            bt_guard.connect().await
                        };
                        
                        match res {
                            Ok(_) => {
                                let config_res = {
                                    let bt_guard = bt_clone.lock().unwrap();
                                    bt_guard.read_config_raw().await
                                };
                                
                                match config_res {
                                    Ok(jsv) => {
                                        let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                        let mut vec = vec![0u8; u8arr.length() as usize];
                                        u8arr.copy_to(&mut vec[..]);
                                        if let Ok(cfg) = postcard::from_bytes::<AppConfig>(&vec) {
                                            let _ = handler_ref.send_message(HandlerMessage::SetConfig(cfg.clone()));
                                            let _ = handler_ref.send_message(HandlerMessage::SetStatus("Connected".into()));
                                            let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Connected(cfg.clone())));
                                            
                                            *hb_run_clone.lock().unwrap() = true;
                                            start_heartbeat(bt_clone.clone(), state_clone.clone(), handler_ref.clone(), hb_run_clone.clone(), ctx.clone());
                                        } else {
                                            let _ = handler_ref.send_message(HandlerMessage::SetStatus("Decode error".into()));
                                            let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Broken(AppConfig::default())));
                                        }
                                    }
                                    Err(e) => {
                                        let _ = handler_ref.send_message(HandlerMessage::SetStatus(format!("Read err: {:?}", e)));
                                        let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Broken(AppConfig::default())));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = handler_ref.send_message(HandlerMessage::SetStatus(format!("Connect err: {:?}", e)));
                                let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Broken(AppConfig::default())));
                            }
                        }
                        
                        let _ = handler_ref.send_message(HandlerMessage::SetBusy(false));
                    });
                }
                
                HandlerMessage::Reload => {
                    let state_clone = state.clone();
                    let bt_clone = bt.clone();
                    let handler_ref = actor_ctx.myself.clone();
                    
                    #[cfg(target_arch = "wasm32")]
                    ractor::concurrency::spawn_local(async move {
                        let _ = handler_ref.send_message(HandlerMessage::SetBusy(true));
                        let _ = handler_ref.send_message(HandlerMessage::SetStatus("Reloading...".into()));
                        
                        let config_res = {
                            let bt_guard = bt_clone.lock().unwrap();
                            bt_guard.read_config_raw().await
                        };
                        
                        match config_res {
                            Ok(jsv) => {
                                let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                let mut vec = vec![0u8; u8arr.length() as usize];
                                u8arr.copy_to(&mut vec[..]);
                                match postcard::from_bytes::<AppConfig>(&vec) {
                                    Ok(cfg) => {
                                        let _ = handler_ref.send_message(HandlerMessage::SetConfig(cfg.clone()));
                                        let _ = handler_ref.send_message(HandlerMessage::SetStatus("Reload OK".into()));
                                    }
                                    Err(e) => {
                                        let _ = handler_ref.send_message(HandlerMessage::SetStatus(format!("Decode error: {:?}", e)));
                                        let current_cfg = state_clone.lock().unwrap().config.clone().unwrap_or_default();
                                        let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Broken(current_cfg)));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = handler_ref.send_message(HandlerMessage::SetStatus(format!("Reload err: {:?}", e)));
                                let current_cfg = state_clone.lock().unwrap().config.clone().unwrap_or_default();
                                let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Broken(current_cfg)));
                            }
                        }
                        
                        let _ = handler_ref.send_message(HandlerMessage::SetBusy(false));
                    });
                }
                
                HandlerMessage::Write => {
                    let state_clone = state.clone();
                    let bt_clone = bt.clone();
                    let handler_ref = actor_ctx.myself.clone();
                    
                    let bytes_res = {
                        let state_guard = state_clone.lock().unwrap();
                        state_guard.config.clone().unwrap_or_default().to_bytes::<1024>()
                    };
                    
                    if let Ok(bytes) = bytes_res {
                        #[cfg(target_arch = "wasm32")]
                        ractor::concurrency::spawn_local(async move {
                            let _ = handler_ref.send_message(HandlerMessage::SetBusy(true));
                            let _ = handler_ref.send_message(HandlerMessage::SetStatus("Writing...".into()));
                            
                            let u8arr = js_sys::Uint8Array::from(&bytes[..]);
                            let write_res = {
                                let bt_guard = bt_clone.lock().unwrap();
                                bt_guard.write_config_raw(&u8arr).await
                            };
                            
                            match write_res {
                                Ok(_) => {
                                    let _ = handler_ref.send_message(HandlerMessage::SetStatus("Write OK".into()));
                                }
                                Err(e) => {
                                    let _ = handler_ref.send_message(HandlerMessage::SetStatus(format!("Write err: {:?}", e)));
                                    let current_cfg = state_clone.lock().unwrap().config.clone().unwrap_or_default();
                                    let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Broken(current_cfg)));
                                }
                            }
                            
                            let _ = handler_ref.send_message(HandlerMessage::SetBusy(false));
                        });
                    } else if let Err(e) = bytes_res {
                        let _ = actor_ctx.myself.send_message(HandlerMessage::SetStatus(format!("Serialize err: {:?}", e)));
                    }
                }
                
                HandlerMessage::Disconnect => {
                    *hb_run.lock().unwrap() = false;
                    let bt_clone = bt.clone();
                    
                    #[cfg(target_arch = "wasm32")]
                    ractor::concurrency::spawn_local(async move {
                        let _ = {
                            let mut bt_guard = bt_clone.lock().unwrap();
                            bt_guard.disconnect().await
                        };
                    });
                    
                    let mut state = state.lock().unwrap();
                    state.conn = ConnectionStatus::Disconnected;
                    state.config = None;
                    state.last_status = "Disconnected".into();
                    state.last_update = Some(Instant::now());
                    ctx_clone.request_repaint();
                }
                
                HandlerMessage::Reconnect => {
                    let state_clone = state.clone();
                    let bt_clone = bt.clone();
                    let handler_ref = actor_ctx.myself.clone();
                    
                    #[cfg(target_arch = "wasm32")]
                    ractor::concurrency::spawn_local(async move {
                        let _ = handler_ref.send_message(HandlerMessage::SetBusy(true));
                        let _ = handler_ref.send_message(HandlerMessage::SetStatus("Reconnecting...".into()));
                        
                        let reconnect_res = {
                            let mut bt_guard = bt_clone.lock().unwrap();
                            bt_guard.reconnect().await
                        };
                        
                        match reconnect_res {
                            Ok(_) => {
                                let config_res = {
                                    let bt_guard = bt_clone.lock().unwrap();
                                    bt_guard.read_config_raw().await
                                };
                                
                                match config_res {
                                    Ok(jsv) => {
                                        let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                        let mut vec = vec![0u8; u8arr.length() as usize];
                                        u8arr.copy_to(&mut vec[..]);
                                        if let Ok(cfg) = postcard::from_bytes::<AppConfig>(&vec) {
                                            let _ = handler_ref.send_message(HandlerMessage::SetConfig(cfg.clone()));
                                            let _ = handler_ref.send_message(HandlerMessage::SetStatus("Connected".into()));
                                            let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Connected(cfg)));
                                        }
                                    }
                                    Err(_) => {
                                        let current_cfg = state_clone.lock().unwrap().config.clone().unwrap_or_default();
                                        let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Connected(current_cfg)));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = handler_ref.send_message(HandlerMessage::SetStatus(format!("Reconnect err: {:?}", e)));
                                let current_cfg = state_clone.lock().unwrap().config.clone().unwrap_or_default();
                                let _ = handler_ref.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Broken(current_cfg)));
                            }
                        }
                        
                        let _ = handler_ref.send_message(HandlerMessage::SetBusy(false));
                    });
                }
            }
        }
    })?;

    Ok(handler)
}

#[cfg(target_arch = "wasm32")]
fn start_heartbeat(
    bt: Arc<Mutex<Bluetooth>>,
    state: Arc<Mutex<AppState>>,
    handler: ActorRef<HandlerMessage>,
    hb_run: Arc<Mutex<bool>>,
    ctx: egui::Context,
) {
    ractor::concurrency::spawn_local(async move {
        let mut interval = gloo_timers::future::IntervalStream::new(5000);
        while (interval.next().await).is_some() {
            if !*hb_run.lock().unwrap() {
                break;
            }
            
            let hb_res = {
                let bt_guard = bt.lock().unwrap();
                bt_guard.heartbeat().await
            };
            
            if let Err(_e) = hb_res {
                let mut reconnected = false;
                for _attempt in 0..3 {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::sleep(Duration::from_millis(1000)).await;
                    
                    let reconnect_res = {
                        let mut bt_guard = bt.lock().unwrap();
                        bt_guard.reconnect().await
                    };
                    
                    if reconnect_res.is_ok() {
                        let _ = handler.send_message(HandlerMessage::SetStatus("Reconnected".into()));
                        reconnected = true;
                        break;
                    }
                }
                
                if !reconnected {
                    let current_cfg = state.lock().unwrap().config.clone().unwrap_or_default();
                    let _ = handler.send_message(HandlerMessage::SetStatus("Connection broken".into()));
                    let _ = handler.send_message(HandlerMessage::SetConnectionStatus(ConnectionStatus::Broken(current_cfg)));
                    ctx.request_repaint();
                    break;
                }
            }
        }
    });
}

// -----------------
// Main App Structure
// -----------------

pub struct PartylightApp {
    state: Arc<Mutex<AppState>>,
    #[cfg(target_arch = "wasm32")]
    handler: ActorRef<HandlerMessage>,
    #[cfg(target_arch = "wasm32")]
    bt: Arc<Mutex<Bluetooth>>,
    #[cfg(target_arch = "wasm32")]
    hb_run: Arc<Mutex<bool>>,
    
    // UI-only state
    styled: bool,
}

impl PartylightApp {
    #[cfg(target_arch = "wasm32")]
    pub fn new(ctx: &egui::Context) -> Result<Self> {
        let state = Arc::new(Mutex::new(AppState {
            config: None,
            last_status: "Idle".to_owned(),
            busy: false,
            conn: ConnectionStatus::Disconnected,
            last_update: None,
        }));
        
        let bt = Arc::new(Mutex::new(Bluetooth::new()));
        let hb_run = Arc::new(Mutex::new(false));
        let handler = create_handler(state.clone(), bt.clone(), hb_run.clone(), ctx.clone())?;
        
        Ok(Self {
            state,
            handler,
            bt,
            hb_run,
            styled: false,
        })
    }
    
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(_ctx: &egui::Context) -> Result<Self> {
        let state = Arc::new(Mutex::new(AppState {
            config: Some(AppConfig::default()),
            last_status: "Native mode".to_owned(),
            busy: false,
            conn: ConnectionStatus::Disconnected,
            last_update: None,
        }));
        
        Ok(Self {
            state,
            styled: false,
        })
    }
}

impl Default for PartylightApp {
    fn default() -> Self {
        // For non-wasm targets
        #[cfg(not(target_arch = "wasm32"))]
        {
            let state = Arc::new(Mutex::new(AppState {
                config: Some(AppConfig::default()),
                last_status: "Native mode".to_owned(),
                busy: false,
                conn: ConnectionStatus::Disconnected,
                last_update: None,
            }));
            
            Self {
                state,
                styled: false,
            }
        }
        
        // For wasm32 target - this should not be used, always use new()
        #[cfg(target_arch = "wasm32")]
        {
            panic!("PartylightApp::default() should not be used on wasm32. Use PartylightApp::new() instead")
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl PartylightApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.styled {
            let mut style = (*ctx.style()).clone();
            style.visuals.extreme_bg_color = colors::BLACK;
            style.visuals.window_fill = colors::BLACK;
            style.visuals.panel_fill = colors::BLACK;
            style.visuals.override_text_color = Some(colors::YELLOW);

            let black = colors::BLACK;
            let pink = colors::PINK;
            let stroke = colors::border_stroke();
            
            style.visuals.widgets.inactive.bg_fill = black;
            style.visuals.widgets.inactive.fg_stroke = stroke;
            style.visuals.widgets.inactive.expansion = 2.0;

            style.visuals.widgets.hovered.bg_fill = pink;
            style.visuals.widgets.hovered.fg_stroke = stroke;

            style.visuals.widgets.active.bg_fill = colors::ACTIVE_PINK;
            style.visuals.widgets.active.fg_stroke = stroke;

            ctx.set_style(style);
            self.styled = true;
        }

        let state = self.state.clone();
        let state_guard = state.lock().unwrap();

        egui::CentralPanel::default().show(ctx, |ui| {
            let painter = ui.painter();
            let rect = ui.max_rect();
            let x = rect.left() + 24.0;
            let y = rect.top() + 18.0;
            let text = "Diskomator 9000 Pro Max Config Editor";
            
            painter.text(
                egui::pos2(x + 4.0, y + 2.0),
                egui::Align2::LEFT_TOP,
                text,
                FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
                Color32::from_rgb(255, 45, 149),
            );
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_TOP,
                text,
                FontId::new(36.0, FontFamily::Name(Arc::from("Cynatar"))),
                Color32::from_rgb(255, 212, 0),
            );

            ui.add_space(64.0);

            match &state_guard.conn {
                ConnectionStatus::Disconnected => {
                    ui.horizontal(|ui| {
                        if ui.add(Button::new("Connect")).clicked() {
                            let _ = self.handler.send_message(HandlerMessage::Connect);
                        }
                    });
                    return;
                }
                ConnectionStatus::Connected(_cfg) => {
                    ui.horizontal(|ui| {
                        ui.label("Connected");
                        if ui.add_enabled(!state_guard.busy, Button::new("Reload")).clicked() {
                            let _ = self.handler.send_message(HandlerMessage::Reload);
                        }
                        if ui.add_enabled(!state_guard.busy, Button::new("Write")).clicked() {
                            let _ = self.handler.send_message(HandlerMessage::Write);
                        }
                        if ui.add_enabled(!state_guard.busy, Button::new("Disconnect")).clicked() {
                            let _ = self.handler.send_message(HandlerMessage::Disconnect);
                        }
                    });
                }
                ConnectionStatus::Broken(_cfg) => {
                    ui.horizontal(|ui| {
                        ui.label("Connection broken");
                        if ui.add_enabled(!state_guard.busy, Button::new("Reconnect")).clicked() {
                            let _ = self.handler.send_message(HandlerMessage::Reconnect);
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

            drop(state_guard);

            let mut state_mut = state.lock().unwrap();
            if let Some(cfg) = &mut state_mut.config {
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

            let mut state_mut = self.state.lock().unwrap();
            if let Some(cfg) = &mut state_mut.config {
                ui.separator();
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
