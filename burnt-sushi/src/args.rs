use std::{lazy::SyncLazy, path::PathBuf};

use clap::{ArgEnum, Parser};

use crate::logger;

pub static ARGS: SyncLazy<Args> = SyncLazy::new(|| {
    // Try to attach console for printing errors during argument parsing.
    logger::raw::attach();

    Args::parse()
});

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Show a console window with debug output.
    #[clap(long)]
    pub console: bool,

    /// Level of debug output.
    #[clap(long, arg_enum, default_value = "debug")]
    pub log_level: LogLevel,

    /// Start a new instance of this app even if one is already running.
    #[clap(long)]
    pub ignore_singleton: bool,

    /// Path to the blocker module.
    /// If the file doesn't exist it will be created with the default blocker.
    /// If not specified the app will try to find it in the same directory as the app with name `burnt-sushi-blocker-x86.dll` or write it to a temp file.
    #[clap(long)]
    pub blocker: Option<PathBuf>,

    /// Path to the filter config.
    /// If the file doesn't exist it will be created with the default config.
    /// If not specified the app will try to find it in the same directory as the app named `filter.toml`.
    #[clap(long)]
    pub filters: Option<PathBuf>,

    #[clap(long, hide = true)]
    pub install: bool,

    #[clap(long, hide = true)]
    pub autostart: bool,
}

#[derive(ArgEnum, Clone, Copy, Debug)]
pub enum LogLevel {
    Off,
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn into_level_filter(self) -> log::LevelFilter {
        match self {
            LogLevel::Off => log::LevelFilter::Off,
            LogLevel::Trace => log::LevelFilter::Trace,
            LogLevel::Debug => log::LevelFilter::Debug,
            LogLevel::Info => log::LevelFilter::Info,
            LogLevel::Warn => log::LevelFilter::Warn,
            LogLevel::Error => log::LevelFilter::Error,
        }
    }
}
