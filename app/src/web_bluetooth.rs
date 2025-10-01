use js_sys::{Array, Function, JsString, Object, Promise, Reflect, Uint8Array};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::window;

const SERVICE_UUID: &str = "bbafe0b7-bf3a-405a-bff7-d632c44c85f8";
const CONFIG_CHAR_UUID: &str = "fa57339a-e7e0-434e-9c98-93a15061e1ff";

thread_local! {
    static DEVICE: RefCell<Option<JsValue>> = RefCell::new(None);
    static SERVER: RefCell<Option<JsValue>> = RefCell::new(None);
    static CFG_CHAR: RefCell<Option<JsValue>> = RefCell::new(None);
}

fn bluetooth_obj() -> Result<JsValue, JsValue> {
    let window = window().ok_or_else(|| JsValue::from_str("no window"))?;
    let nav = window.navigator();
    Reflect::get(&nav, &JsValue::from_str("bluetooth"))
}

async fn request_device_with_options(opts: &JsValue) -> Result<JsValue, JsValue> {
    let bt = bluetooth_obj()?;
    let req = Reflect::get(&bt, &JsValue::from_str("requestDevice"))?;
    let func: Function = req.dyn_into()?;
    let promise: Promise = func.call1(&bt, opts)?.dyn_into()?;
    Ok(JsFuture::from(promise).await?)
}

async fn connect_gatt(device: &JsValue) -> Result<JsValue, JsValue> {
    let gatt = Reflect::get(device, &JsValue::from_str("gatt"))?;
    let conn_fn = Reflect::get(&gatt, &JsValue::from_str("connect"))?;
    let func: Function = conn_fn.dyn_into()?;
    let promise: Promise = func.call0(&gatt)?.dyn_into()?;
    Ok(JsFuture::from(promise).await?)
}

async fn get_service(server: &JsValue, uuid: &str) -> Result<JsValue, JsValue> {
    let get_fn = Reflect::get(server, &JsValue::from_str("getPrimaryService"))?;
    let func: Function = get_fn.dyn_into()?;
    let promise: Promise = func.call1(server, &JsValue::from_str(uuid))?.dyn_into()?;
    Ok(JsFuture::from(promise).await?)
}

async fn get_characteristic(service: &JsValue, uuid: &str) -> Result<JsValue, JsValue> {
    let get_fn = Reflect::get(service, &JsValue::from_str("getCharacteristic"))?;
    let func: Function = get_fn.dyn_into()?;
    let promise: Promise = func.call1(service, &JsValue::from_str(uuid))?.dyn_into()?;
    Ok(JsFuture::from(promise).await?)
}

pub async fn connect_to_device() -> Result<(), JsValue> {
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

    let device = match request_device_with_options(&opts.into()).await {
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
            Reflect::set(
                &opts2,
                &JsValue::from_str("optionalServices"),
                &Array::of1(&JsValue::from_str(SERVICE_UUID)),
            )?;
            match request_device_with_options(&opts2.into()).await {
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
                    request_device_with_options(&opts3.into()).await?
                }
            }
        }
    };

    // store device
    DEVICE.with(|d| *d.borrow_mut() = Some(device.clone()));

    // connect
    let server = connect_gatt(&device).await?;
    SERVER.with(|s| *s.borrow_mut() = Some(server.clone()));

    // get service and characteristic
    let service = get_service(&server, SERVICE_UUID).await?;
    let cfg = get_characteristic(&service, CONFIG_CHAR_UUID).await?;
    CFG_CHAR.with(|c| *c.borrow_mut() = Some(cfg));

    // attach disconnect event to device to clear cached state
    // device.addEventListener('gattserverdisconnected', ...)
    let on_disc = Closure::wrap(Box::new(move |_ev: JsValue| {
        DEVICE.with(|d| *d.borrow_mut() = None);
        SERVER.with(|s| *s.borrow_mut() = None);
        CFG_CHAR.with(|c| *c.borrow_mut() = None);
    }) as Box<dyn FnMut(JsValue)>);
    let _ = Reflect::get(&device, &JsValue::from_str("addEventListener")).and_then(|add| {
        let func: Function = add.dyn_into().unwrap();
        let _ = func.call2(
            &device,
            &JsValue::from_str("gattserverdisconnected"),
            on_disc.as_ref().unchecked_ref(),
        );
        Ok(())
    });
    on_disc.forget();

    Ok(())
}

pub async fn read_config() -> Result<Uint8Array, JsValue> {
    // ensure we have cfg char
    let mut have = false;
    CFG_CHAR.with(|c| have = c.borrow().is_some());
    if !have {
        // try reconnect non-interactively by reusing device if possible
        DEVICE.with(|_d| { /* noop: we could attempt a silent reconnect here */ });
        connect_to_device().await?;
    }
    let char = CFG_CHAR
        .with(|c| c.borrow().clone())
        .ok_or_else(|| JsValue::from_str("Not connected"))?;
    let read_fn = Reflect::get(&char, &JsValue::from_str("readValue"))?;
    let func: Function = read_fn.dyn_into()?;
    let promise: Promise = func.call0(&char)?.dyn_into()?;
    let v = JsFuture::from(promise).await?;
    // v is a DataView-like, get buffer
    let buffer = Reflect::get(&v, &JsValue::from_str("buffer"))?;
    Ok(Uint8Array::new(&buffer))
}

pub async fn write_config(data: &Uint8Array) -> Result<(), JsValue> {
    let mut have = false;
    CFG_CHAR.with(|c| have = c.borrow().is_some());
    if !have {
        connect_to_device().await?;
    }
    let char = CFG_CHAR
        .with(|c| c.borrow().clone())
        .ok_or_else(|| JsValue::from_str("Not connected"))?;
    let write_fn = Reflect::get(&char, &JsValue::from_str("writeValue"))?;
    let func: Function = write_fn.dyn_into()?;
    let _promise: Promise = func.call1(&char, data)?.dyn_into()?;
    // await completion
    let _ = JsFuture::from(_promise).await?;
    Ok(())
}
