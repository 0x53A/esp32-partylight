use crate::config::*;


impl AppConfig {
    pub fn stripes() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            sample_count: 256,
            fft_size: FFTSize::Size512,
            use_hann_window: false,
            pattern: NeopixelMatrixPattern::Stripes([
                ChannelConfig {
                    start_index: 1,
                    end_index: 1,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [255, 0, 0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 2,
                    end_index: 10,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0, 255, 0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 11,
                    end_index: 15,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0, 0, 255],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 16,
                    end_index: 25,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [255, 255, 255],
                    aggregate: AggregationMethod::Sum,
                },
            ]),
        }

    }


    pub fn bars() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            sample_count: 256,
            fft_size: FFTSize::Size512,
            use_hann_window: false,
            pattern: NeopixelMatrixPattern::Bars([
                ChannelConfig {
                    start_index: 1,
                    end_index: 2,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [255, 0, 0], // Red
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 3,
                    end_index: 4,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [255, 127, 0], // Orange
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 5,
                    end_index: 7,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [255, 255, 0], // Yellow
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 8,
                    end_index: 10,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0, 255, 0], // Green
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 11,
                    end_index: 14,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0, 255, 255], // Cyan
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 15,
                    end_index: 18,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0, 0, 255], // Blue
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 19,
                    end_index: 22,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [127, 0, 255], // Purple
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 23,
                    end_index: 25,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [255, 0, 255], // Magenta
                    aggregate: AggregationMethod::Sum,
                },
            ]),
        }

    }


    pub fn quarters() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            sample_count: 256,
            fft_size: FFTSize::Size512,
            use_hann_window: false,
            pattern: NeopixelMatrixPattern::Quarters([
                ChannelConfig {
                    start_index: 1,
                    end_index: 4,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [255, 0, 0],
                    aggregate: AggregationMethod::Sum,

                },
                ChannelConfig {
                    start_index: 5,
                    end_index: 10,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0, 255, 0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 11,
                    end_index: 15,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0, 0, 255],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 16,
                    end_index: 25,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [255, 255, 255],
                    aggregate: AggregationMethod::Sum,
                },
            ]),
        }

    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::bars()
    }
}