use std::{
    fmt::Debug,
    sync::{Mutex, MutexGuard},
};

use chrono::Local;

use log::Log;

use crate::APP_NAME;

use super::{ConsoleLog, FileLog, SimpleLog};

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
    logger.console = ConsoleLog::new();
    logger.file = None;
}

#[derive(Debug)]
pub struct GlobalLoggerHolder(Mutex<GlobalLogger>);

#[derive(Debug)]
pub struct GlobalLogger {
    pub console: ConsoleLog,
    pub file: Option<FileLog>,
}

impl GlobalLogger {
    pub const fn new() -> Self {
        GlobalLogger {
            console: ConsoleLog::new(),
            file: None,
        }
    }

    pub fn has_console(&self) -> bool {
        self.console.is_active()
    }

    #[allow(dead_code)]
    pub fn has_log_file(&self) -> bool {
        self.file.is_some()
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
        let message = format!("[{}] {}", record.level(), record.args());
        if let Err(e) = logger.console.log(&message) {
            fallback_log!("Failed to write to console ({e:#}): {message}");
        }
        if let Some(file) = &mut logger.file {
            let date_time = Local::now().format("%Y-%m-%d %H:%M:%S");
            let message = format!("{} [{}] {}", date_time, record.level(), record.args());
            if let Err(e) = file.log(&message) {
                fallback_log!("Failed to write to log file ({e:#}): {message}");
                logger.file = None;
            }
        }
    }

    fn flush(&self) {}
}
