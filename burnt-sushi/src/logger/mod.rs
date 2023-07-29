pub mod global;
pub mod console;
pub mod file;
pub mod noop;

mod traits;

pub use traits::*;
pub use console::Console;
pub use file::FileLog;
