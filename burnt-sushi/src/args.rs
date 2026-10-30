use std::{
    env,
    ffi::OsString,
    path::PathBuf,
    sync::{LazyLock, OnceLock},
};

use clap::{Parser, ValueEnum, error::ContextKind};

use crate::logger;

pub static IGNORED_ARGS: OnceLock<Vec<String>> = OnceLock::new();

pub static ARGS: LazyLock<Args> = LazyLock::new(|| {
    // Try to attach console for printing errors during argument parsing.
    logger::console::raw::attach();

    let (args, ignored) = parse_known_args();
    let _ = IGNORED_ARGS.set(ignored);
    args
});

fn parse_known_args() -> (Args, Vec<String>) {
    let mut argv: Vec<OsString> = env::args_os().collect();
    let mut ignored = Vec::new();

    loop {
        match Args::try_parse_from(&argv) {
            Ok(args) => return (args, ignored),
            Err(err) if err.kind() == clap::error::ErrorKind::UnknownArgument => {
                let bad_arg = err
                    .context()
                    .find(|(kind, _)| *kind == ContextKind::InvalidArg)
                    .map(|(_, value)| value.to_string());

                let Some(bad_arg) = bad_arg else {
                    err.exit();
                };
                let Some(pos) = argv.iter().position(|arg| *arg == *bad_arg) else {
                    err.exit();
                };
                argv.remove(pos);
                ignored.push(bad_arg);
            }
            Err(err) => err.exit(),
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None, args_override_self = true)]
#[allow(clippy::struct_excessive_bools)]
pub struct Args {
    /// Show a console window with log output, see `--console-log-level`.
    #[arg(long)]
    pub console: bool,

    /// Do not attach to a parent console to show log output.
    #[arg(long)]
    #[arg(conflicts_with("console"))]
    pub no_attach: bool,

    /// Level of debug output.
    #[arg(long, value_enum, default_value = "debug")]
    pub log_level: LogLevel,

    /// Level of output shown in the console window, which is kept to a readable summary
    /// by default. Use `debug` to also see every request. Never shows more than
    /// `--log-level` lets through.
    #[arg(long, value_enum, default_value = "info")]
    pub console_log_level: LogLevel,

    /// Path to a log file to write to.
    #[arg(long)]
    pub log_file: Option<PathBuf>,

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
    pub singleton_wait_for_shutdown: bool,

    #[arg(long, hide = true)]
    pub autostart: bool,

    #[arg(long, hide = true)]
    pub force_restart: bool,

    /// Force an update check, bypassing the metered-connection skip and the once-a-week throttle.
    #[arg(long)]
    pub check_for_update: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
