#![feature(once_cell, maybe_uninit_uninit_array, maybe_uninit_slice)]
#![warn(unsafe_op_in_unsafe_fn)]
#![allow(clippy::module_inception, non_snake_case)]
#![windows_subsystem = "windows"]

use log::{debug, error, info, trace, warn};
use std::env;

use crate::{args::ARGS, blocker::SpotifyAdBlocker, logger::Console, named_mutex::NamedMutex};

mod args;
mod blocker;
mod logger;
mod named_mutex;
mod resolver;
mod rpc;
mod spotify_process_scanner;
mod tray;

const APP_NAME: &str = "BurntSushi";
const APP_AUTHOR: &str = "OpenByteDev";
// const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_NAME_WITH_VERSION: &str = concat!("BurntSushi v", env!("CARGO_PKG_VERSION"));
const DEFAULT_BLOCKER_FILE_NAME: &str = "BurntSushiBlocker_x86.dll";
const DEFAULT_FILTER_FILE_NAME: &str = "filter.toml";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    logger::global::init();

    if let Some(console) = Console::attach() {
        logger::global::get().set_console(console);
    }

    if ARGS.console {
        if let Some(console) = Console::alloc() {
            logger::global::get().set_console(console);
        }
    }

    log::set_max_level(ARGS.log_level.into_level_filter());

    info!("{}", APP_NAME_WITH_VERSION);
    trace!(
        "Running from {}",
        env::current_exe()
            .unwrap_or_else(|_| "<unknown>".into())
            .display()
    );

    if ARGS.install {
        if !is_elevated::is_elevated() {
            error!("Must be run as administrator.");
            std::process::exit(1);
        }

        let current_location = env::current_exe().unwrap();
        let blocker_location = current_location
            .parent()
            .unwrap()
            .join(DEFAULT_BLOCKER_FILE_NAME);
        resolver::resolve_blocker(Some(&blocker_location))
            .await
            .unwrap();
        return;
    }

    if ARGS.ignore_singleton {
        run().await;
    } else {
        let lock = NamedMutex::new(&format!("{} SINGLETON MUTEX", APP_NAME)).unwrap();
        match lock.try_lock() {
            Ok(Some(_guard)) => run().await,
            Ok(None) => {
                error!("App is already running. (use --ignore-singleton to ignore)\nExiting...")
            }
            Err(e) => error!(
                "Failed to lock singleton mutex: {} (use --ignore-singleton to ignore)  ",
                e
            ),
        };
    }

    logger::global::unset();
}

async fn run() {
    let mut system_tray = tray::SystemTrayManager::build_and_run().await.unwrap();

    let mut app = SpotifyAdBlocker::new();

    tokio::select! {
        _ = app.run() => {
            unreachable!("App should never exit on its own");
        }
        _ = wait_for_ctrl_c() => {
            debug!("Ctrl-C received");
        }
        _ = system_tray.wait_for_exit() => {
            debug!("System tray exited");
        }
    }

    info!("Shutting down...");

    app.stop().await;
    system_tray.exit().await;

    info!("Exiting...");
}

async fn wait_for_ctrl_c() -> Result<(), ctrlc::Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut handler = Some(move || tx.send(()).unwrap());
    ctrlc::set_handler(move || {
        if let Some(h) = handler.take() {
            h()
        }
    })?;
    rx.await.unwrap();
    Ok(())
}
