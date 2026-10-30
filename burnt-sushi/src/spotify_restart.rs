//! Restarts an already running Spotify instance during startup.
//!
//! The hooks only cover requests made after the blocker is injected, so an instance that
//! was already up has resolved hosts and created requests we never got to filter.
//! Restarting it puts the blocker in place before Spotify issues its first request.
//!
//! This interrupts playback, so it only happens when enabled with `--restart-spotify`.

use std::{
    io,
    os::windows::prelude::FromRawHandle,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr,
    time::{Duration, Instant},
};

use anyhow::Context;
use dll_syringe::process::{OwnedProcess, Process};
use log::{debug, info, warn};
use widestring::u16cstr;
use windows_sys::Win32::{
    Foundation::FALSE,
    System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE},
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

use crate::spotify_process_scanner::is_spotify_process;

/// How long to wait for the old processes to disappear before giving up on them.
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// One of the standard access rights, needed to wait on the process handle.
/// `windows-sys` only exposes it as a file access right, which would mean pulling in
/// `Win32_Storage_FileSystem` for a single constant.
const SYNCHRONIZE: u32 = 0x0010_0000;

/// Terminates every running Spotify process and starts a fresh one.
///
/// Returns `Ok(false)` when Spotify was not running, in which case nothing was done.
pub async fn restart_running_spotify() -> anyhow::Result<bool> {
    let processes = find_spotify_processes();
    if processes.is_empty() {
        debug!("Spotify is not running, nothing to restart");
        return Ok(false);
    }

    info!("Found {} running Spotify process(es)", processes.len());

    // Read the path before terminating, it is no longer available afterwards.
    let exe_path = spotify_exe_path(&processes);

    let terminating = terminate_all(&processes);
    if let Err(e) = wait_for_exit(&terminating).await {
        warn!("{e:#}, starting Spotify anyway");
    }

    launch(exe_path.as_deref())?;

    Ok(true)
}

fn find_spotify_processes() -> Vec<OwnedProcess> {
    OwnedProcess::all()
        .into_iter()
        .filter(|process| is_spotify_process(process.borrowed()))
        .collect()
}

/// Picks the executable to relaunch. The renderer and GPU processes run the same binary as
/// the main one, so any of them will do, but prefer `Spotify.exe` over a helper process.
fn spotify_exe_path(processes: &[OwnedProcess]) -> Option<PathBuf> {
    let paths: Vec<PathBuf> = processes
        .iter()
        .filter_map(|process| process.path().ok())
        .collect();

    paths
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("Spotify.exe"))
        })
        .or_else(|| paths.first())
        .cloned()
}

/// Reopens a process with the rights needed to terminate it and to observe it exiting.
///
/// The handles returned by process enumeration only carry query rights, so calling
/// `kill` on them fails with `ERROR_ACCESS_DENIED`.
fn open_for_termination(pid: u32) -> io::Result<OwnedProcess> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            FALSE,
            pid,
        )
    };

    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }

    Ok(unsafe { OwnedProcess::from_raw_handle(handle) })
}

/// Terminates every given process, returning handles that can be used to wait for them.
fn terminate_all(processes: &[OwnedProcess]) -> Vec<OwnedProcess> {
    let mut terminating = Vec::with_capacity(processes.len());

    for process in processes {
        let Ok(pid) = process.pid() else {
            continue;
        };
        let pid = pid.get();

        let handle = match open_for_termination(pid) {
            Ok(handle) => handle,
            Err(e) => {
                // Killing the main process takes the renderer and GPU processes with it,
                // so by the time we get here a sibling may already be gone.
                if process.is_alive() {
                    warn!("Failed to open Spotify process (PID={pid}): {e}");
                }
                continue;
            }
        };

        match handle.kill() {
            Ok(()) => {
                debug!("Terminated Spotify process (PID={pid})");
                terminating.push(handle);
            }
            Err(_) if !handle.is_alive() => {
                debug!("Spotify process (PID={pid}) had already exited");
            }
            Err(e) => warn!("Failed to terminate Spotify process (PID={pid}): {e}"),
        }
    }

    terminating
}

async fn wait_for_exit(processes: &[OwnedProcess]) -> anyhow::Result<()> {
    let deadline = Instant::now() + EXIT_TIMEOUT;

    while processes.iter().any(|process| process.is_alive()) {
        if Instant::now() >= deadline {
            anyhow::bail!("Spotify did not exit within {EXIT_TIMEOUT:?}");
        }
        tokio::time::sleep(EXIT_POLL_INTERVAL).await;
    }

    debug!("All Spotify processes exited");
    Ok(())
}

fn launch(exe_path: Option<&Path>) -> anyhow::Result<()> {
    if let Some(path) = exe_path {
        debug!("Starting Spotify from '{}'", path.display());
        match Command::new(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => {
                info!("Restarted Spotify");
                return Ok(());
            }
            Err(e) => warn!("Failed to start Spotify from '{}': {e}", path.display()),
        }
    }

    debug!("Starting Spotify via the 'spotify:' protocol handler");
    launch_via_protocol().context("Failed to start Spotify")?;

    info!("Restarted Spotify");
    Ok(())
}

/// Fallback for Microsoft Store installs, which live in a locked down `WindowsApps`
/// directory that cannot be launched by path.
fn launch_via_protocol() -> anyhow::Result<()> {
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            u16cstr!("open").as_ptr(),
            u16cstr!("spotify:").as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    // `ShellExecuteW` reports success with a value greater than 32.
    if result as isize <= 32 {
        return Err(io::Error::last_os_error()).context("ShellExecuteW failed");
    }

    Ok(())
}
