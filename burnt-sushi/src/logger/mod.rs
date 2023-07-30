pub mod console;
pub mod file;
pub mod global;
pub mod noop;

mod traits;

pub use console::Console;
pub use file::FileLog;
pub use traits::*;
