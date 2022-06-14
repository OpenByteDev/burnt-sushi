#![feature(
    io_safety,
    once_cell,
    maybe_uninit_uninit_array,
    maybe_uninit_slice,
    let_chains
)]
#![warn(unsafe_op_in_unsafe_fn)]
#![allow(clippy::module_inception, non_snake_case)]
#![windows_subsystem = "windows"]

use log::{debug, error, info, warn};
use std::env;

use crate::{args::ARGS, blocker::SpotifyAdBlocker, console::Console, named_mutex::NamedMutex};

mod args;
mod blocker;
mod console;
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
    if ARGS.console {
        console::global::set(
            Console::attach()
                .or_else(Console::alloc)
                .unwrap_or_else(Console::none),
        );
    } else {
        console::global::set(Console::none());
    }

    log::set_max_level(ARGS.log_level.into_level_filter());

    info!("{}", APP_NAME_WITH_VERSION);

    if ARGS.ignore_singleton {
        run().await;
    } else {
        let lock = NamedMutex::new(&format!("{} SINGLETON MUTEX", APP_NAME)).unwrap();
        match lock.try_lock() {
            Ok(Some(_guard)) => run().await,
            Ok(None) => error!("App is already running.\nExiting..."),
            Err(e) => error!("Failed to lock singleton mutex: {}", e),
        };
    }

    console::global::unset();
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
