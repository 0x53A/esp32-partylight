use crate::config::*;

impl AppConfig {
    pub fn stripes() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            sample_count: 256,
            fft_size: FFTSize::Size512,
            use_hann_window: true,
            pattern: NeopixelMatrixPattern::Stripes([
                ChannelConfig {
                    start_index: 1,
                    end_index: 1,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [1.0, 0.0, 0.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 2,
                    end_index: 10,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0.0, 1.0, 0.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 11,
                    end_index: 15,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0.0, 0.0, 1.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 16,
                    end_index: 25,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [1.0, 1.0, 1.0],
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
            use_hann_window: true,
            pattern: NeopixelMatrixPattern::Bars([
                ChannelConfig {
                    start_index: 1,
                    end_index: 2,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [1.0, 0.0, 0.0], // Red
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 3,
                    end_index: 4,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [1.0, 0.498, 0.0], // Orange
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 5,
                    end_index: 7,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [1.0, 1.0, 0.0], // Yellow
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 8,
                    end_index: 10,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0.0, 1.0, 0.0], // Green
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 11,
                    end_index: 14,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0.0, 1.0, 1.0], // Cyan
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 15,
                    end_index: 18,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0.0, 0.0, 1.0], // Blue
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 19,
                    end_index: 22,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0.498, 0.0, 1.0], // Purple
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 23,
                    end_index: 25,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [1.0, 0.0, 1.0], // Magenta
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
            use_hann_window: true,
            pattern: NeopixelMatrixPattern::Quarters([
                ChannelConfig {
                    start_index: 1,
                    end_index: 4,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [1.0, 0.0, 0.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 5,
                    end_index: 10,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0.0, 1.0, 0.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 11,
                    end_index: 15,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [0.0, 0.0, 1.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 16,
                    end_index: 25,
                    premult: 3.0,
                    noise_gate: 0.01,
                    exponent: 6,
                    color: [1.0, 1.0, 1.0],
                    aggregate: AggregationMethod::Sum,
                },
            ]),
        }
    }
}

impl AppConfig {
    pub fn bars2() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            sample_count: 256,
            fft_size: FFTSize::Size512,
            use_hann_window: true,
            pattern: NeopixelMatrixPattern::Bars([
                ChannelConfig {
                    start_index: 1,
                    end_index: 1,
                    premult: 2.0,
                    noise_gate: 0.0,
                    exponent: 1,
                    color: [1.0, 0.0, 0.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 2,
                    end_index: 3,
                    premult: 3.0,
                    noise_gate: 0.0,
                    exponent: 1,
                    color: [1.0, 0.498, 0.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 4,
                    end_index: 5,
                    premult: 3.0,
                    noise_gate: 0.0,
                    exponent: 1,
                    color: [1.0, 1.0, 0.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 6,
                    end_index: 10,
                    premult: 5.0,
                    noise_gate: 0.0,
                    exponent: 1,
                    color: [0.0, 1.0, 0.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 11,
                    end_index: 14,
                    premult: 10.0,
                    noise_gate: 0.0,
                    exponent: 1,
                    color: [0.0, 1.0, 1.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 15,
                    end_index: 18,
                    premult: 10.0,
                    noise_gate: 0.0,
                    exponent: 1,
                    color: [0.0, 0.0, 1.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 19,
                    end_index: 22,
                    premult: 10.0,
                    noise_gate: 0.0,
                    exponent: 1,
                    color: [0.498, 0.0, 1.0],
                    aggregate: AggregationMethod::Sum,
                },
                ChannelConfig {
                    start_index: 23,
                    end_index: 100,
                    premult: 10.0,
                    noise_gate: 0.0,
                    exponent: 1,
                    color: [1.0, 0.0, 1.0],
                    aggregate: AggregationMethod::Sum,
                },
            ]),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::bars2()
    }
}
