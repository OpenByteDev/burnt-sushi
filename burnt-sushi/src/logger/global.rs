use std::sync::OnceLock;

use super::{Console, Logger};

static LOGGER: OnceLock<Logger> = OnceLock::new();

pub fn init() -> &'static Logger {
    let logger = LOGGER.get_or_init(|| Logger::new(Console::none()));
    let _ = log::set_logger(logger);
    logger
}

pub fn get() -> &'static Logger {
    LOGGER.get().unwrap_or_else(init)
}

pub fn unset() {
    get().set_console(Console::none())
}
