use common::config::*;
use egui::{self, Button, Color32};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use js_sys;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;

// Import JS helper functions as returning Promises; we'll await them via JsFuture.
#[cfg(target_arch = "wasm32")]
use crate::web_bluetooth as web_bt;

pub struct AliasApp {
    config: AppConfig,
    last_status: String,
}

impl Default for AliasApp {
    fn default() -> Self {
        Self {
            config: AppConfig {
                config_version: CONFIG_VERSION,
                sample_count: 32,
                fft_size: FFTSize::Size256,
                use_hann_window: true,
                pattern: NeopixelMatrixPattern::Stripes([
                    ChannelConfig {
                        start_index: 0,
                        end_index: 3,
                        premult: 1.0,
                        noise_gate: 0.0,
                        exponent: 1,
                        color: [255, 0, 0],
                        aggregate: AggregationMethod::Sum,
                    },
                    ChannelConfig {
                        start_index: 4,
                        end_index: 7,
                        premult: 1.0,
                        noise_gate: 0.0,
                        exponent: 1,
                        color: [0, 255, 0],
                        aggregate: AggregationMethod::Sum,
                    },
                    ChannelConfig {
                        start_index: 8,
                        end_index: 11,
                        premult: 1.0,
                        noise_gate: 0.0,
                        exponent: 1,
                        color: [0, 0, 255],
                        aggregate: AggregationMethod::Sum,
                    },
                    ChannelConfig {
                        start_index: 12,
                        end_index: 15,
                        premult: 1.0,
                        noise_gate: 0.0,
                        exponent: 1,
                        color: [255, 255, 0],
                        aggregate: AggregationMethod::Sum,
                    },
                ]),
            },
            last_status: "Idle".to_owned(),
        }
    }
}
#[cfg(target_arch = "wasm32")]
impl AliasApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Blindomator 9000 Pro Max Config Editor");
            ui.label(&self.last_status);

            ui.horizontal(|ui| {
                if ui.add(Button::new("Connect")).clicked() {
                    self.last_status = "Connecting...".into();
                    let last_status = ui.ctx().clone();
                    let app_ptr: *mut AliasApp = self as *mut _;
                    wasm_bindgen_futures::spawn_local(async move {
                        match web_bt::connect_to_device().await {
                            Ok(_) => {
                                unsafe {
                                    (*app_ptr).last_status = "Connected".into();
                                }
                                last_status.request_repaint();
                            }
                            Err(e) => {
                                unsafe {
                                    (*app_ptr).last_status = format!("Connect error: {:?}", e);
                                }
                                last_status.request_repaint();
                            }
                        }
                    });
                }

                if ui.add(Button::new("Read from device")).clicked() {
                    self.last_status = "Reading...".into();
                    let last_status = ui.ctx().clone();
                    let app_ptr: *mut AliasApp = self as *mut _;
                    wasm_bindgen_futures::spawn_local(async move {
                        match web_bt::read_config().await {
                            Ok(jsv) => {
                                let u8arr = js_sys::Uint8Array::new(&jsv.into());
                                let mut vec = vec![0u8; u8arr.length() as usize];
                                u8arr.copy_to(&mut vec[..]);
                                match postcard::from_bytes::<AppConfig>(&vec) {
                                    Ok(cfg) => unsafe {
                                        (*app_ptr).config = cfg;
                                    },
                                    Err(e) => unsafe {
                                        (*app_ptr).last_status = format!("Decode error: {:?}", e);
                                    },
                                }
                                unsafe {
                                    (*app_ptr).last_status = "Read OK".into();
                                }
                                last_status.request_repaint();
                            }
                            Err(e) => unsafe {
                                (*app_ptr).last_status = format!("Read err: {:?}", e);
                                last_status.request_repaint();
                            },
                        }
                    });
                }

                if ui.add(Button::new("Write to device")).clicked() {
                    self.last_status = "Writing...".into();
                    let last_status = ui.ctx().clone();
                    let app_ptr: *mut AliasApp = self as *mut _;
                    let bytes = match self.config.to_bytes::<1024>() {
                        Ok(b) => b,
                        Err(e) => {
                            self.last_status = format!("Serialize err: {:?}", e);
                            return;
                        }
                    };
                    let u8arr = js_sys::Uint8Array::from(&bytes[..]);
                    wasm_bindgen_futures::spawn_local(async move {
                        match web_bt::write_config(&js_sys::Uint8Array::from(&bytes[..])).await {
                            Ok(_) => unsafe {
                                (*app_ptr).last_status = "Write OK".into();
                            },
                            Err(e) => unsafe {
                                (*app_ptr).last_status = format!("Write err: {:?}", e);
                            },
                        }
                        last_status.request_repaint();
                    });
                }
            });

            ui.separator();

            ui.label("Basic settings:");
            ui.horizontal(|ui| {
                ui.label("Sample count:");
                let mut sc = self.config.sample_count as u32;
                if ui.add(egui::widgets::DragValue::new(&mut sc)).changed() {
                    self.config.sample_count = sc as usize;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Use Hann window:");
                ui.checkbox(&mut self.config.use_hann_window, "");
            });

            ui.separator();

            ui.label("Pattern:");
            // Pattern selector
            let mut pattern_idx = match &self.config.pattern {
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

            // If user changed the combo selection, convert the pattern preserving channel data where possible
            let selected_pattern_idx = pattern_idx;
            // If different, convert
            match (selected_pattern_idx, &mut self.config.pattern) {
                (0, NeopixelMatrixPattern::Stripes(_)) => {}
                (0, other) => {
                    // convert other -> Stripes (4 channels)
                    let mut new: [ChannelConfig; 4] = std::array::from_fn(|_| ChannelConfig {
                        start_index: 0,
                        end_index: 0,
                        premult: 1.0,
                        noise_gate: 0.0,
                        exponent: 1,
                        color: [255, 255, 255],
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
                    self.config.pattern = NeopixelMatrixPattern::Stripes(new);
                }
                (1, NeopixelMatrixPattern::Bars(_)) => {}
                (1, other) => {
                    // convert to Bars (8 channels)
                    let mut new: [ChannelConfig; 8] = std::array::from_fn(|_| ChannelConfig {
                        start_index: 0,
                        end_index: 0,
                        premult: 1.0,
                        noise_gate: 0.0,
                        exponent: 1,
                        color: [255, 255, 255],
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
                    self.config.pattern = NeopixelMatrixPattern::Bars(new);
                }
                (2, NeopixelMatrixPattern::Quarters(_)) => {}
                (2, other) => {
                    // convert to Quarters (4 channels)
                    let mut new: [ChannelConfig; 4] = std::array::from_fn(|_| ChannelConfig {
                        start_index: 0,
                        end_index: 0,
                        premult: 1.0,
                        noise_gate: 0.0,
                        exponent: 1,
                        color: [255, 255, 255],
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
                    self.config.pattern = NeopixelMatrixPattern::Quarters(new);
                }
                _ => {}
            }

            // Render editor for whichever variant is active
            match &mut self.config.pattern {
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
                                let mut r = ch.color[0] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut r));
                                ch.color[0] = r;
                                let mut g = ch.color[1] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut g));
                                ch.color[1] = g;
                                let mut b = ch.color[2] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut b));
                                ch.color[2] = b;
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
                                let mut r = ch.color[0] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut r));
                                ch.color[0] = r;
                                let mut g = ch.color[1] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut g));
                                ch.color[1] = g;
                                let mut b = ch.color[2] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut b));
                                ch.color[2] = b;
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
                                let mut r = ch.color[0] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut r));
                                ch.color[0] = r;
                                let mut g = ch.color[1] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut g));
                                ch.color[1] = g;
                                let mut b = ch.color[2] as u8;
                                ui.add(egui::widgets::DragValue::new(&mut b));
                                ch.color[2] = b;
                            });
                        });
                    }
                }
            }
        });
    }
}

// Provide a native (non-wasm) UI stub so the app can still run natively.
#[cfg(not(target_arch = "wasm32"))]
impl AliasApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Blindomator 9000 Pro Max Config Editor (WASM only)");
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
