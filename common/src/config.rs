use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum AggregationMethod {
    Sum,
    Max,
    Average,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChannelConfig {
    /// index into the FFT array, inclusive
    pub start_index: usize,
    /// index into the FFT array, inclusive
    pub end_index: usize,

    pub premult: f32,
    pub noise_gate: f32,
    pub exponent: u8,
    /// RGB color for this channel (0.0 - 1.0)
    pub color: [f32; 3],
    pub aggregate: AggregationMethod,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NeopixelMatrixPattern {
    Stripes([ChannelConfig; 4]),
    Bars([ChannelConfig; 8]),
    Quarters([ChannelConfig; 4]),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FFTSize {
    Size128 = 128,
    Size256 = 256,
    Size512 = 512,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub config_version: u32,
    pub sample_count: usize,
    pub fft_size: FFTSize,
    pub use_hann_window: bool,
    pub pattern: NeopixelMatrixPattern,
}

pub const CONFIG_VERSION: u32 = 1;

impl AppConfig {
    /// Serialize config to binary data using postcard
    pub fn to_bytes<const B: usize>(&self) -> postcard::Result<heapless::Vec<u8, B>> {
        let result = postcard::to_vec::<_, B>(self);
        result
    }

    /// Deserialize config from binary data using postcard
    pub fn from_bytes(data: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(data)
    }
}
