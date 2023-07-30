#![feature(
    once_cell_try,
    lazy_cell,
    maybe_uninit_uninit_array,
    maybe_uninit_slice,
    io_error_other,
    iter_intersperse
)]
#![warn(unsafe_op_in_unsafe_fn)]
#![allow(clippy::module_inception, non_snake_case)]
#![windows_subsystem = "windows"]

use anyhow::{anyhow, Context};
use dll_syringe::process::{OwnedProcess, Process};
use log::{debug, error, info, trace, warn};
use winapi::{
    shared::minwindef::FALSE,
    um::{processthreadsapi::OpenProcess, synchapi::WaitForSingleObject, winnt::PROCESS_TERMINATE},
};

use std::{env, io, os::windows::prelude::FromRawHandle, time::Duration};

use crate::{args::{ARGS, LogLevel}, blocker::SpotifyAdBlocker, named_mutex::NamedMutex, logger::{Console, FileLog}};

mod args;
mod blocker;
mod logger;
mod named_mutex;
mod resolver;
mod rpc;
mod spotify_process_scanner;
mod tray;
mod update;

const APP_NAME: &str = "BurntSushi";
const APP_AUTHOR: &str = "OpenByteDev";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_NAME_WITH_VERSION: &str = concat!("BurntSushi v", env!("CARGO_PKG_VERSION"));
const DEFAULT_BLOCKER_FILE_NAME: &str = "BurntSushiBlocker_x64.dll";
const DEFAULT_FILTER_FILE_NAME: &str = "filter.toml";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    logger::global::init();

    log::set_max_level(ARGS.log_level.into_level_filter());

    if let Some(console) = Console::attach() {
        logger::global::get().console = Some(console);
        debug!("Attached to console");
    }

    if ARGS.console {
        if let Some(console) = Console::alloc() {
            logger::global::get().console = Some(console);
            debug!("Allocated new console");
        }
    }

    let mut log_file = ARGS.log_file.clone();
    if log_file.is_none() && ARGS.log_level == LogLevel::Debug {
        let mut auto_log_file = dirs::data_dir();
        if let Some(ref mut log_file) = auto_log_file {
            log_file.push("OpenByte");
            log_file.push("BurntSushi");
            log_file.push("BurntSushi.log");
        }
        log_file = auto_log_file;
    }
    if let Some(log_file) = log_file {
        logger::global::get().file = Some(FileLog::new(log_file));
    }

    info!("{}", APP_NAME_WITH_VERSION);
    trace!(
        "Running from {}",
        env::current_exe()
            .unwrap_or_else(|_| "<unknown>".into())
            .display()
    );

    if ARGS.install {
        match handle_install().await {
            Ok(()) => info!("App successfully installed."),
            Err(e) => error!("Failed to install application: {e}"),
        }
        return;
    }

    if let Some(old_bin_path) = &ARGS.update_old_bin {
        tokio::task::spawn(tokio::fs::remove_file(old_bin_path));
    }

    if ARGS.force_restart {
        match terminate_other_instances() {
            Ok(_) => debug!("Killed previously running instances"),
            Err(err) => {
                error!("Failed to open previously running instance (err={err})");
                return;
            }
        }
    }

    if ARGS.ignore_singleton {
        run().await;
    } else {
        let lock = NamedMutex::new(&format!("{APP_NAME} SINGLETON MUTEX")).unwrap();

        let mut guard_result = lock.try_lock();

        if ARGS.singleton_wait_for_shutdown {
            while matches!(guard_result, Ok(None)) {
                tokio::time::sleep(Duration::from_millis(100)).await;
                guard_result = lock.try_lock();
            }
        }

        match guard_result {
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

    let (update_restart_tx, update_restart_rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn(async move {
        match update::update().await {
            Ok(true) => update_restart_tx.send(()).unwrap(),
            Ok(false) => {}
            Err(e) => error!("App update failed: {e:#}"),
        }
    });

    tokio::select! {
        _ = app.run() => {
        }
        _ = wait_for_ctrl_c() => {
            debug!("Ctrl-C received");
        }
        _ = system_tray.wait_for_exit() => {
            debug!("System tray exited");
        }
        Ok(_) = update_restart_rx => {
            debug!("Shutting down due to update");
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

async fn handle_install() -> anyhow::Result<()> {
    if !is_elevated::is_elevated() {
        return Err(anyhow!("Must be run as administrator"));
    }

    let current_location = env::current_exe().context("Failed to locate current executable")?;
    let blocker_location = current_location
        .parent()
        .ok_or_else(|| anyhow!("Failed to determine parent directory"))?
        .join(DEFAULT_BLOCKER_FILE_NAME);
    resolver::resolve_blocker(Some(&blocker_location))
        .await
        .context("Failed to write blocker to disk")?;

    Ok(())
}

fn terminate_other_instances() -> anyhow::Result<()> {
    let other_processes = OwnedProcess::find_all_by_name(APP_NAME)
        .into_iter()
        .filter(|p| !p.is_current());

    for process in other_processes {
        let handle = unsafe {
            OpenProcess(
                PROCESS_TERMINATE,
                FALSE,
                process.pid().map_or(0, |v| v.get()),
            )
        };

        if handle.is_null() {
            Err(io::Error::last_os_error()).context("Failed to access other running instances")?;
        }

        let process = unsafe { OwnedProcess::from_raw_handle(handle) };
        process.kill().context("Failed to kill process.")?;

        let _ = unsafe { WaitForSingleObject(handle, Duration::from_secs(5).as_millis() as _) };
    }

    Ok(())
}
