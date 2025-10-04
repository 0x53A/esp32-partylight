use alloc::string::String;
use log::{Metadata, Record};

use core::fmt::Write;

use anyhow::Result;

use rtt_target::rprintln;

#[macro_export]
macro_rules! error_with_location {
    ($msg:expr) => {
        anyhow!("{} at {}:{}", $msg, file!(), line!())
    };
    ($fmt:expr, $($arg:tt)*) => {
        anyhow!("{} at {}:{}", format!($fmt, $($arg)*), file!(), line!())
    };
}

pub struct MultiLogger;

impl log::Log for MultiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        // format once
        let mut buf = String::new();
        let _ = write!(
            &mut buf,
            "[{}] {}: {}",
            record.level(),
            record.target(),
            record.args()
        );

        // RTT
        rprintln!("{}", buf);

        // UART — use esp_println::println! which writes directly to UART (avoid log! macros here
        // to prevent recursion)
        esp_println::println!("{}", buf);
    }

    fn flush(&self) {}
}

#[macro_use]
mod static_cell_helpers {
    #[macro_export]
    macro_rules! static_cell_init {
        ($ty:ty, $init:expr) => {{
            {
                static STATIC_CELL: ::static_cell::StaticCell<$ty> =
                    ::static_cell::StaticCell::new();
                STATIC_CELL.init($init)
            }
        }};
    }

    #[macro_export]
    macro_rules! static_buf {
        ($ty:ty, $size:expr) => {{
            {
                static mut BUF: [$ty; $size] = [0; $size];
                #[allow(static_mut_refs)]
                #[allow(unsafe_code)]
                unsafe {
                    &mut BUF
                }
            }
        }};
    }
}
