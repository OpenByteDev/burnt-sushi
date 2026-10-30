use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{Mutex, MutexGuard},
};

use chrono::Local;

use log::{LevelFilter, Log};

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

/// How many recent console lines are kept around to replay into a console opened later.
const HISTORY_CAPACITY: usize = 250;

#[derive(Debug)]
pub struct GlobalLogger {
    pub console: Option<Console>,
    pub file: Option<FileLog>,
    console_level: LevelFilter,
    history: VecDeque<String>,
}

impl GlobalLogger {
    pub const fn new() -> Self {
        GlobalLogger {
            console: None,
            file: None,
            console_level: LevelFilter::Info,
            history: VecDeque::new(),
        }
    }

    pub fn set_console_level(&mut self, level: LevelFilter) {
        self.console_level = level;
    }

    /// Attaches a console, replaying the recent log into it.
    ///
    /// A console opened in the middle of a session would otherwise stay empty until the
    /// next event, hiding everything that happened during startup.
    pub fn set_console(&mut self, mut console: Console) {
        for message in &self.history {
            console.log(message);
        }
        self.console = Some(console);
    }

    fn record_history(&mut self, message: String) {
        if self.history.len() == HISTORY_CAPACITY {
            self.history.pop_front();
        }
        self.history.push_back(message);
    }

    /// Returns whether a usable console is attached, dropping it first if it went away.
    ///
    /// A piped console outlives the window it writes to, so without this the console
    /// would stay set forever once the user closes it.
    pub fn has_active_console(&mut self) -> bool {
        if self.console.as_ref().is_some_and(Console::is_active) {
            return true;
        }

        self.console = None;
        false
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

        // The console is a human readable summary, the file keeps the full detail.
        if record.level() <= logger.console_level {
            let message = format!("[{}] {}", record.level(), record.args());
            if logger.has_active_console() {
                if let Some(log) = &mut logger.console {
                    log.log(&message);
                }
            }
            logger.record_history(message);
        }

        if let Some(log) = &mut logger.file {
            let date_time = Local::now().format("%Y-%m-%d %H:%M:%S");
            let message = format!("{} [{}] {}", date_time, record.level(), record.args());
            log.log(&message);
        }
    }

    fn flush(&self) {}
}
