use std::{path::PathBuf, sync::LazyLock};

use clap::{Parser, ValueEnum};

use crate::logger;

pub static ARGS: LazyLock<Args> = LazyLock::new(|| {
    // Try to attach console for printing errors during argument parsing.
    logger::raw::attach();

    Args::parse()
});

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Show a console window with debug output.
    #[arg(long)]
    pub console: bool,

    /// Level of debug output.
    #[arg(long, value_enum, default_value = "debug")]
    pub log_level: LogLevel,

    /// Start a new instance of this app even if one is already running.
    #[arg(long)]
    pub ignore_singleton: bool,

    /// Exit program once spotify is closed, will wait for spotify to start if not currently running.
    #[arg(long)]
    pub shutdown_with_spotify: bool,

    /// Path to the blocker module.
    /// If the file doesn't exist it will be created with the default blocker.
    /// If not specified the app will try to find it in the same directory as the app with name `burnt-sushi-blocker-x86.dll` or write it to a temp file.
    #[arg(long)]
    pub blocker: Option<PathBuf>,

    /// Path to the filter config.
    /// If the file doesn't exist it will be created with the default config.
    /// If not specified the app will try to find it in the same directory as the app named `filter.toml`.
    #[arg(long)]
    pub filters: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub install: bool,

    #[arg(long, hide = false)]
    pub update_old_bin: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub update_elevate_restart: bool,

    #[arg(long, hide = true)]
    pub singleton_wait_for_shutdown: bool,

    #[arg(long, hide = true)]
    pub autostart: bool,

    #[arg(long, hide = true)]
    pub force_restart: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
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
