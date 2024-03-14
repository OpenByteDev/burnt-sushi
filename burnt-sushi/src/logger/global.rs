use std::{
    fmt::Debug,
    sync::{Mutex, MutexGuard}
};

use chrono::Local;

use log::Log;

use crate::APP_NAME;

use super::{Console, FileLog, SimpleLog};

static LOGGER: GlobalLoggerHolder = GlobalLoggerHolder(Mutex::new(GlobalLogger::new()));

pub fn init() -> &'static GlobalLoggerHolder {
    let _ = log::set_logger(&LOGGER);
    &LOGGER
}

pub fn get() -> MutexGuard<'static, GlobalLogger> {
    LOGGER.0.lock().unwrap()
}

pub fn unset() {
    let mut logger = get();
    logger.console = None;
    logger.file = None;
}

#[derive(Debug)]
pub struct GlobalLoggerHolder(Mutex<GlobalLogger>);

#[derive(Debug)]
pub struct GlobalLogger {
    pub console: Option<Console>,
    pub file: Option<FileLog>,
}

impl GlobalLogger {
    pub const fn new() -> Self {
        GlobalLogger {
            console: None,
            file: None,
        }
    }
}

impl Log for GlobalLoggerHolder {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if !record.target().starts_with(APP_NAME) {
            return;
        }

        let mut logger = self.0.lock().unwrap();
        if let Some(log) = &mut logger.console {
            let message = format!("[{}] {}", record.level(), record.args());
            log.log(&message);
        }
        if let Some(log) = &mut logger.file {
            let date_time = Local::now().format("%Y-%m-%d %H:%M:%S");
            let message = format!("{} [{}] {}", date_time, record.level(), record.args());
            log.log(&message);
        }
    }

    fn flush(&self) {}
}
