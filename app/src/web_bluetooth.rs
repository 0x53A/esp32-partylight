use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, console};

const SERVICE_UUID: &str = "bbafe0b7-bf3a-405a-bff7-d632c44c85f8";
const CONFIG_CHAR_UUID: &str = "fa57339a-e7e0-434e-9c98-93a15061e1ff";

// OTA Service UUIDs
const OTA_SERVICE_UUID: &str = "c6e7a9f0-1b34-4c5d-8f6e-2a3b4c5d6e7f";
const OTA_CONTROL_CHAR_UUID: &str = "d7f8b0e1-2c45-5d6e-9f7a-3b4c5d6e7f80";
const OTA_DATA_CHAR_UUID: &str = "e8f9c1d2-3d56-6e7f-a08b-4c5d6e7f8091";
const OTA_STATUS_CHAR_UUID: &str = "f9d0e2c3-4e67-7f80-b19c-5d6e7f809102";

pub struct Bluetooth {
    device: Option<JsValue>,
    server: Option<JsValue>,
    cfg_char: Option<JsValue>,
    ota_control_char: Option<JsValue>,
    ota_data_char: Option<JsValue>,
    ota_status_char: Option<JsValue>,
}

impl Bluetooth {
    pub fn new() -> Self {
        Self {
            device: None,
            server: None,
            cfg_char: None,
            ota_control_char: None,
            ota_data_char: None,
            ota_status_char: None,
        }
    }

    fn bluetooth_obj() -> Result<JsValue, JsValue> {
        let window = window().ok_or_else(|| JsValue::from_str("no window"))?;
        let nav = window.navigator();
        console::log_1(&JsValue::from_str("web_bluetooth: getting navigator.bluetooth"));
        Reflect::get(&nav, &JsValue::from_str("bluetooth"))
    }

    async fn request_device_with_options(opts: &JsValue) -> Result<JsValue, JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: request_device_with_options start"));
        let bt = Self::bluetooth_obj()?;
        let req = Reflect::get(&bt, &JsValue::from_str("requestDevice"))?;
        let func: Function = req.dyn_into()?;
        let promise: Promise = func.call1(&bt, opts)?.dyn_into()?;
        let result = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: request_device_with_options success"));
        Ok(result)
    }

    async fn connect_gatt(device: &JsValue) -> Result<JsValue, JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: connect_gatt start"));
        let gatt = Reflect::get(device, &JsValue::from_str("gatt"))?;
        let conn_fn = Reflect::get(&gatt, &JsValue::from_str("connect"))?;
        let func: Function = conn_fn.dyn_into()?;
        let promise: Promise = func.call0(&gatt)?.dyn_into()?;
        let res = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: connect_gatt success"));
        Ok(res)
    }

    async fn get_service(server: &JsValue, uuid: &str) -> Result<JsValue, JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: get_service start"));
        let get_fn = Reflect::get(server, &JsValue::from_str("getPrimaryService"))?;
        let func: Function = get_fn.dyn_into()?;
        let promise: Promise = func.call1(server, &JsValue::from_str(uuid))?.dyn_into()?;
        let res = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: get_service success"));
        Ok(res)
    }

    async fn get_characteristic(service: &JsValue, uuid: &str) -> Result<JsValue, JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: get_characteristic start"));
        let get_fn = Reflect::get(service, &JsValue::from_str("getCharacteristic"))?;
        let func: Function = get_fn.dyn_into()?;
        let promise: Promise = func.call1(service, &JsValue::from_str(uuid))?.dyn_into()?;
        let res = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: get_characteristic success"));
        Ok(res)
    }

    // Connect interactively (requestDevice) and populate internal fields
    pub async fn connect(&mut self) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: connect start"));
        // Try service-based filter first
        let opts = Object::new();
        let filters = Array::new();
        let f = Object::new();
        Reflect::set(
            &f,
            &JsValue::from_str("services"),
            &Array::of1(&JsValue::from_str(SERVICE_UUID)),
        )?;
        filters.push(&f);
        Reflect::set(&opts, &JsValue::from_str("filters"), &filters)?;
        
        // Include both config service and OTA service in optionalServices
        let optional_services = Array::new();
        optional_services.push(&JsValue::from_str(SERVICE_UUID));
        optional_services.push(&JsValue::from_str(OTA_SERVICE_UUID));
        Reflect::set(
            &opts,
            &JsValue::from_str("optionalServices"),
            &optional_services,
        )?;

        let device = match Self::request_device_with_options(&opts.into()).await {
            Ok(dev) => dev,
            Err(_) => {
                // fallback to namePrefix
                let opts2 = Object::new();
                let filters2 = Array::new();
                let f2 = Object::new();
                Reflect::set(
                    &f2,
                    &JsValue::from_str("namePrefix"),
                    &JsValue::from_str("Blindomator"),
                )?;
                filters2.push(&f2);
                Reflect::set(&opts2, &JsValue::from_str("filters"), &filters2)?;
                
                let optional_services2 = Array::new();
                optional_services2.push(&JsValue::from_str(SERVICE_UUID));
                optional_services2.push(&JsValue::from_str(OTA_SERVICE_UUID));
                Reflect::set(
                    &opts2,
                    &JsValue::from_str("optionalServices"),
                    &optional_services2,
                )?;
                match Self::request_device_with_options(&opts2.into()).await {
                    Ok(dev) => dev,
                    Err(_) => {
                        // final fallback: acceptAllDevices
                        let opts3 = Object::new();
                        Reflect::set(
                            &opts3,
                            &JsValue::from_str("acceptAllDevices"),
                            &JsValue::from_bool(true),
                        )?;
                        let optional_services3 = Array::new();
                        optional_services3.push(&JsValue::from_str(SERVICE_UUID));
                        optional_services3.push(&JsValue::from_str(OTA_SERVICE_UUID));
                        Reflect::set(
                            &opts3,
                            &JsValue::from_str("optionalServices"),
                            &optional_services3,
                        )?;
                        Self::request_device_with_options(&opts3.into()).await?
                    }
                }
            }
        };


    // store device
    console::log_1(&JsValue::from_str("web_bluetooth: device selected"));
    self.device = Some(device.clone());

    // connect
    console::log_1(&JsValue::from_str("web_bluetooth: connecting gatt"));
    let server = Self::connect_gatt(&device).await?;
    console::log_1(&JsValue::from_str("web_bluetooth: gatt connected"));
        self.server = Some(server.clone());

        // get config service and characteristic
        console::log_1(&JsValue::from_str("web_bluetooth: getting config service"));
        let service = Self::get_service(&server, SERVICE_UUID).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: getting config characteristic"));
        let cfg = Self::get_characteristic(&service, CONFIG_CHAR_UUID).await?;
        self.cfg_char = Some(cfg);

        // get OTA service and characteristics
        console::log_1(&JsValue::from_str("web_bluetooth: getting OTA service"));
        match Self::get_service(&server, OTA_SERVICE_UUID).await {
            Ok(ota_service) => {
                console::log_1(&JsValue::from_str("web_bluetooth: getting OTA control characteristic"));
                let ota_control = Self::get_characteristic(&ota_service, OTA_CONTROL_CHAR_UUID).await?;
                self.ota_control_char = Some(ota_control);
                
                console::log_1(&JsValue::from_str("web_bluetooth: getting OTA data characteristic"));
                let ota_data = Self::get_characteristic(&ota_service, OTA_DATA_CHAR_UUID).await?;
                self.ota_data_char = Some(ota_data);
                
                console::log_1(&JsValue::from_str("web_bluetooth: getting OTA status characteristic"));
                let ota_status = Self::get_characteristic(&ota_service, OTA_STATUS_CHAR_UUID).await?;
                self.ota_status_char = Some(ota_status);
                
                console::log_1(&JsValue::from_str("web_bluetooth: OTA service initialized"));
            }
            Err(e) => {
                console::log_1(&JsValue::from_str("web_bluetooth: OTA service not available (might be older firmware)"));
                console::log_1(&e);
            }
        }

        console::log_1(&JsValue::from_str("web_bluetooth: connect complete"));
        Ok(())
    }

    // Try to reconnect non-interactively by using existing device object (if any)
    pub async fn reconnect(&mut self) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: reconnect start"));
        let device = self.device.as_ref().ok_or_else(|| JsValue::from_str("No device cached"))?;
        let server = Self::connect_gatt(device).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: reconnect gatt connected"));
        self.server = Some(server.clone());
        
        let service = Self::get_service(&server, SERVICE_UUID).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: reconnect got config service"));
        let cfg = Self::get_characteristic(&service, CONFIG_CHAR_UUID).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: reconnect got config characteristic"));
        self.cfg_char = Some(cfg);
        
        // Try to get OTA service
        match Self::get_service(&server, OTA_SERVICE_UUID).await {
            Ok(ota_service) => {
                let ota_control = Self::get_characteristic(&ota_service, OTA_CONTROL_CHAR_UUID).await?;
                self.ota_control_char = Some(ota_control);
                let ota_data = Self::get_characteristic(&ota_service, OTA_DATA_CHAR_UUID).await?;
                self.ota_data_char = Some(ota_data);
                let ota_status = Self::get_characteristic(&ota_service, OTA_STATUS_CHAR_UUID).await?;
                self.ota_status_char = Some(ota_status);
                console::log_1(&JsValue::from_str("web_bluetooth: reconnect got OTA service"));
            }
            Err(_) => {
                console::log_1(&JsValue::from_str("web_bluetooth: OTA service not available on reconnect"));
            }
        }
        
        console::log_1(&JsValue::from_str("web_bluetooth: reconnect complete"));
        Ok(())
    }

    pub async fn read_config_raw(&self) -> Result<Uint8Array, JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: read_config_raw start"));
        let char = self.cfg_char.as_ref().ok_or_else(|| JsValue::from_str("Not connected"))?;
        let read_fn = Reflect::get(char, &JsValue::from_str("readValue"))?;
        let func: Function = read_fn.dyn_into()?;
        let promise: Promise = func.call0(char)?.dyn_into()?;
        let v = JsFuture::from(promise).await?;
        let buffer = Reflect::get(&v, &JsValue::from_str("buffer"))?;
        console::log_1(&JsValue::from_str("web_bluetooth: read_config_raw success"));
        Ok(Uint8Array::new(&buffer))
    }

    pub async fn write_config_raw(&self, data: &Uint8Array) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: write_config_raw start"));
        let char = self.cfg_char.as_ref().ok_or_else(|| JsValue::from_str("Not connected"))?;
        let write_fn = Reflect::get(char, &JsValue::from_str("writeValue"))?;
        let func: Function = write_fn.dyn_into()?;
        let promise: Promise = func.call1(char, data)?.dyn_into()?;
        let _ = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: write_config_raw success"));
        Ok(())
    }

    // Heartbeat: do a small read to keep the GATT connection alive
    pub async fn heartbeat(&self) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: heartbeat start"));
        let _ = self.read_config_raw().await?;
        console::log_1(&JsValue::from_str("web_bluetooth: heartbeat success"));
        Ok(())
    }

    /// Attempt to disconnect and clear cached handles.
    pub async fn disconnect(&mut self) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: disconnect start"));
        // Try to call disconnect on the cached server or device.gatt
        if let Some(srv) = self.server.take() {
            if let Ok(disc) = Reflect::get(&srv, &JsValue::from_str("disconnect")) {
                if let Ok(func) = disc.dyn_into::<Function>() {
                    let _ = func.call0(&srv);
                    console::log_1(&JsValue::from_str("web_bluetooth: server.disconnect called"));
                }
            }
        }
        // try device.gatt.disconnect() as fallback
        if let Some(dev) = self.device.take() {
            if let Ok(gatt) = Reflect::get(&dev, &JsValue::from_str("gatt")) {
                if let Ok(disc) = Reflect::get(&gatt, &JsValue::from_str("disconnect")) {
                    if let Ok(func) = disc.dyn_into::<Function>() {
                        let _ = func.call0(&gatt);
                        console::log_1(&JsValue::from_str("web_bluetooth: device.gatt.disconnect called"));
                    }
                }
            }
        }

        // clear characteristic as well
        self.cfg_char = None;
        self.ota_control_char = None;
        self.ota_data_char = None;
        self.ota_status_char = None;
        self.server = None;
        self.device = None;
        console::log_1(&JsValue::from_str("web_bluetooth: disconnect complete"));
        Ok(())
    }

    /// Begin OTA update
    pub async fn ota_begin(&self) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: ota_begin start"));
        let char = self.ota_control_char.as_ref().ok_or_else(|| JsValue::from_str("OTA not available"))?;
        let cmd = Uint8Array::new_with_length(1);
        cmd.set_index(0, 0x01); // OTA_CMD_BEGIN
        
        let write_fn = Reflect::get(char, &JsValue::from_str("writeValue"))?;
        let func: Function = write_fn.dyn_into()?;
        let promise: Promise = func.call1(char, &cmd)?.dyn_into()?;
        let _ = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: ota_begin success"));
        Ok(())
    }

    /// Write firmware data chunk
    pub async fn ota_write_chunk(&self, data: &Uint8Array) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str(&format!("web_bluetooth: ota_write_chunk ({} bytes)", data.length())));
        let char = self.ota_data_char.as_ref().ok_or_else(|| JsValue::from_str("OTA not available"))?;
        
        let write_fn = Reflect::get(char, &JsValue::from_str("writeValue"))?;
        let func: Function = write_fn.dyn_into()?;
        let promise: Promise = func.call1(char, data)?.dyn_into()?;
        let _ = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: ota_write_chunk success"));
        Ok(())
    }

    /// Commit OTA update (device will reboot)
    pub async fn ota_commit(&self) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: ota_commit start"));
        let char = self.ota_control_char.as_ref().ok_or_else(|| JsValue::from_str("OTA not available"))?;
        let cmd = Uint8Array::new_with_length(1);
        cmd.set_index(0, 0x02); // OTA_CMD_COMMIT
        
        let write_fn = Reflect::get(char, &JsValue::from_str("writeValue"))?;
        let func: Function = write_fn.dyn_into()?;
        let promise: Promise = func.call1(char, &cmd)?.dyn_into()?;
        let _ = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: ota_commit success"));
        Ok(())
    }

    /// Abort OTA update
    pub async fn ota_abort(&self) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: ota_abort start"));
        let char = self.ota_control_char.as_ref().ok_or_else(|| JsValue::from_str("OTA not available"))?;
        let cmd = Uint8Array::new_with_length(1);
        cmd.set_index(0, 0x03); // OTA_CMD_ABORT
        
        let write_fn = Reflect::get(char, &JsValue::from_str("writeValue"))?;
        let func: Function = write_fn.dyn_into()?;
        let promise: Promise = func.call1(char, &cmd)?.dyn_into()?;
        let _ = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: ota_abort success"));
        Ok(())
    }

    /// Read OTA status
    pub async fn ota_read_status(&self) -> Result<u8, JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: ota_read_status start"));
        let char = self.ota_status_char.as_ref().ok_or_else(|| JsValue::from_str("OTA not available"))?;
        
        let read_fn = Reflect::get(char, &JsValue::from_str("readValue"))?;
        let func: Function = read_fn.dyn_into()?;
        let promise: Promise = func.call0(char)?.dyn_into()?;
        let v = JsFuture::from(promise).await?;
        let buffer = Reflect::get(&v, &JsValue::from_str("buffer"))?;
        let arr = Uint8Array::new(&buffer);
        
        let status = if arr.length() > 0 {
            arr.get_index(0)
        } else {
            0
        };
        
        console::log_1(&JsValue::from_str(&format!("web_bluetooth: ota_read_status = {}", status)));
        Ok(status)
    }
}
