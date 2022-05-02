use std::sync::{Mutex, MutexGuard};

use log::Log;

use crate::APP_NAME;

use super::Console;

#[derive(Debug)]
pub struct Logger {
    console: Mutex<Console>,
}

impl Logger {
    pub fn new(console: Console) -> Self {
        Logger {
            console: Mutex::new(console),
        }
    }

    pub fn get(&self) -> MutexGuard<'_, Console> {
        self.console.lock().unwrap()
    }
}

impl Log for Logger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if !record.target().starts_with(APP_NAME) {
            return;
        }

        let message = format!("[{}] {}", record.level(), record.args());
        self.console.lock().unwrap().println(message).unwrap();
    }

    fn flush(&self) {}
}
