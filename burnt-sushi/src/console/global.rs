use std::{lazy::SyncOnceCell, sync::MutexGuard};

use super::{Console, Logger};

static LOGGER: SyncOnceCell<Logger> = SyncOnceCell::new();

pub fn set(console: Console) {
    let logger = LOGGER.get_or_init(|| Logger::new(console));
    let _ = log::set_logger(logger);
}

pub fn get() -> MutexGuard<'static, Console> {
    LOGGER.get().unwrap().get()
}

pub fn unset() {
    *get() = Console::none()
}
