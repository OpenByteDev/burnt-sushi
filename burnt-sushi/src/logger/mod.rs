/// Last resort for messages that could not be written to their log.
macro_rules! fallback_log {
    ($($arg:tt)*) => {
        eprintln!($($arg)*)
    };
}

pub mod console;
pub mod file;
pub mod global;
pub mod noop;

mod traits;

pub use console::ConsoleLog;
pub use file::FileLog;
pub use traits::*;
