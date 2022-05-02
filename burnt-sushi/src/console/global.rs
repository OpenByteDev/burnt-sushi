use std::{lazy::SyncOnceCell, sync::MutexGuard};

use log::LevelFilter;

use super::{Console, Logger};

static LOGGER: SyncOnceCell<Logger> = SyncOnceCell::new();

pub fn set(console: Console) {
    let logger = LOGGER.get_or_init(|| Logger::new(console));
    let _ = log::set_logger(logger);
    log::set_max_level(LevelFilter::Trace);
}

pub fn get() -> MutexGuard<'static, Console> {
    LOGGER.get().unwrap().get()
}

pub fn unset() {
    *get() = Console::none()
}
