use std::collections::VecDeque;

use crate::console::Console;

use super::SimpleLog;

const HISTORY_LEN: usize = 250;

#[derive(Debug)]
pub struct ConsoleLog {
    console: Option<Console>,
    history: VecDeque<String>,
}

impl ConsoleLog {
    pub const fn new() -> Self {
        Self {
            console: None,
            history: VecDeque::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.console.as_ref().is_some_and(Console::is_active)
    }

    pub fn attach(&mut self, mut console: Console) {
        for message in &self.history {
            if let Err(e) = console.println(message) {
                fallback_log!("Failed to write to console ({e:#}): {message}");
                return;
            }
        }
        self.console = Some(console);
    }
}

impl SimpleLog for ConsoleLog {
    fn log(&mut self, message: &str) -> anyhow::Result<()> {
        if self.history.len() == HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(message.to_string());

        let Some(console) = &mut self.console else {
            return Ok(());
        };
        console
            .println(message)
            .inspect_err(|_| self.console = None)?;
        Ok(())
    }
}
