use std::{collections::VecDeque, sync::Mutex};

use log::Log;

use crate::APP_NAME;

use super::Console;

#[derive(Debug)]
pub struct Logger {
    console: Mutex<LoggerInner>,
}

#[derive(Debug)]
struct LoggerInner {
    console: Console,
    log: VecDeque<String>,
}

impl Logger {
    pub fn new(console: Console) -> Self {
        Logger {
            console: Mutex::new(LoggerInner {
                console,
                log: VecDeque::new(),
            }),
        }
    }

    pub fn has_console(&self) -> bool {
        self.console.lock().unwrap().console.is_active()
    }

    #[allow(dead_code)]
    pub fn with_console(&self, f: impl FnOnce(&mut Console)) {
        f(&mut self.console.lock().unwrap().console)
    }

    pub fn set_console(&self, console: Console) {
        let mut inner = self.console.lock().unwrap();
        inner.console = console;
        for message in &inner.log {
            inner.console.println(message).unwrap();
        }
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
        let mut inner = self.console.lock().unwrap();
        inner.console.println(&message).unwrap();
        inner.log.push_back(message);
        if inner.log.len() > 1000 {
            inner.log.pop_front();
        }
    }

    fn flush(&self) {}
}
