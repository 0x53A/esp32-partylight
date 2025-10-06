use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{console, window};

const SERVICE_UUID: &str = "bbafe0b7-bf3a-405a-bff7-d632c44c85f8";
const CONFIG_CHAR_UUID: &str = "fa57339a-e7e0-434e-9c98-93a15061e1ff";

pub struct Bluetooth {
    device: Option<JsValue>,
    server: Option<JsValue>,
    cfg_char: Option<JsValue>,
}

impl Bluetooth {
    pub fn new() -> Self {
        Self {
            device: None,
            server: None,
            cfg_char: None,
        }
    }

    fn bluetooth_obj() -> Result<JsValue, JsValue> {
        let window = window().ok_or_else(|| JsValue::from_str("no window"))?;
        let nav = window.navigator();
        console::log_1(&JsValue::from_str(
            "web_bluetooth: getting navigator.bluetooth",
        ));
        Reflect::get(&nav, &JsValue::from_str("bluetooth"))
    }

    async fn request_device_with_options(opts: &JsValue) -> Result<JsValue, JsValue> {
        console::log_1(&JsValue::from_str(
            "web_bluetooth: request_device_with_options start",
        ));
        let bt = Self::bluetooth_obj()?;
        let req = Reflect::get(&bt, &JsValue::from_str("requestDevice"))?;
        let func: Function = req.dyn_into()?;
        let promise: Promise = func.call1(&bt, opts)?.dyn_into()?;
        let result = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str(
            "web_bluetooth: request_device_with_options success",
        ));
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
        console::log_1(&JsValue::from_str(
            "web_bluetooth: get_characteristic start",
        ));
        let get_fn = Reflect::get(service, &JsValue::from_str("getCharacteristic"))?;
        let func: Function = get_fn.dyn_into()?;
        let promise: Promise = func.call1(service, &JsValue::from_str(uuid))?.dyn_into()?;
        let res = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str(
            "web_bluetooth: get_characteristic success",
        ));
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
        Reflect::set(
            &opts,
            &JsValue::from_str("optionalServices"),
            &Array::of1(&JsValue::from_str(SERVICE_UUID)),
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
                    &JsValue::from_str("Diskomator"),
                )?;
                filters2.push(&f2);
                Reflect::set(&opts2, &JsValue::from_str("filters"), &filters2)?;
                Reflect::set(
                    &opts2,
                    &JsValue::from_str("optionalServices"),
                    &Array::of1(&JsValue::from_str(SERVICE_UUID)),
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
                        Reflect::set(
                            &opts3,
                            &JsValue::from_str("optionalServices"),
                            &Array::of1(&JsValue::from_str(SERVICE_UUID)),
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

        // get service and characteristic
        console::log_1(&JsValue::from_str("web_bluetooth: getting service"));
        let service = Self::get_service(&server, SERVICE_UUID).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: getting characteristic"));
        let cfg = Self::get_characteristic(&service, CONFIG_CHAR_UUID).await?;
        self.cfg_char = Some(cfg);

        console::log_1(&JsValue::from_str("web_bluetooth: connect complete"));
        Ok(())
    }

    // Try to reconnect non-interactively by using existing device object (if any)
    pub async fn reconnect(&mut self) -> Result<(), JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: reconnect start"));
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No device cached"))?;
        let server = Self::connect_gatt(device).await?;
        console::log_1(&JsValue::from_str(
            "web_bluetooth: reconnect gatt connected",
        ));
        self.server = Some(server.clone());
        let service = Self::get_service(&server, SERVICE_UUID).await?;
        console::log_1(&JsValue::from_str("web_bluetooth: reconnect got service"));
        let cfg = Self::get_characteristic(&service, CONFIG_CHAR_UUID).await?;
        console::log_1(&JsValue::from_str(
            "web_bluetooth: reconnect got characteristic",
        ));
        self.cfg_char = Some(cfg);
        console::log_1(&JsValue::from_str("web_bluetooth: reconnect complete"));
        Ok(())
    }

    pub async fn read_config_raw(&self) -> Result<Uint8Array, JsValue> {
        console::log_1(&JsValue::from_str("web_bluetooth: read_config_raw start"));
        let char = self
            .cfg_char
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Not connected"))?;
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
        let char = self
            .cfg_char
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Not connected"))?;
        let write_fn = Reflect::get(char, &JsValue::from_str("writeValue"))?;
        let func: Function = write_fn.dyn_into()?;
        let promise: Promise = func.call1(char, data)?.dyn_into()?;
        let _ = JsFuture::from(promise).await?;
        console::log_1(&JsValue::from_str(
            "web_bluetooth: write_config_raw success",
        ));
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
                    console::log_1(&JsValue::from_str(
                        "web_bluetooth: server.disconnect called",
                    ));
                }
            }
        }
        // try device.gatt.disconnect() as fallback
        if let Some(dev) = self.device.take() {
            if let Ok(gatt) = Reflect::get(&dev, &JsValue::from_str("gatt")) {
                if let Ok(disc) = Reflect::get(&gatt, &JsValue::from_str("disconnect")) {
                    if let Ok(func) = disc.dyn_into::<Function>() {
                        let _ = func.call0(&gatt);
                        console::log_1(&JsValue::from_str(
                            "web_bluetooth: device.gatt.disconnect called",
                        ));
                    }
                }
            }
        }

        // clear characteristic as well
        self.cfg_char = None;
        self.server = None;
        self.device = None;
        console::log_1(&JsValue::from_str("web_bluetooth: disconnect complete"));
        Ok(())
    }
}
