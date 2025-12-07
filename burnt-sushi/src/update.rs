use std::{
    env,
    io::{self, Write},
    path::Path,
    process::Stdio,
    ptr,
};

use anyhow::Context;
use log::{debug, error, info};
use reqwest::header::HeaderValue;
use self_update::update::Release;
use tokio::fs::{self, File};
use u16cstr::u16cstr;
use widestring::U16CString;
use winapi::um::{shellapi::ShellExecuteW, winuser::SW_SHOWDEFAULT};
use winrt_toast::{Action, Text, Toast, ToastManager};

use crate::{APP_NAME, APP_VERSION, ARGS};

pub async fn update() -> anyhow::Result<bool> {
    let releases = tokio::task::spawn_blocking(load_releases)
        .await
        .context("Failed to load releases")?
        .context("Failed to load releases")?;

    let (release, release_version) = releases
        .into_iter()
        .filter_map(|r| lenient_semver::parse(&r.version).ok().map(|v| (r, v)))
        .max_by(|(_, v1), (_, v2)| v1.cmp(v2))
        .context("No valid release found")?;

    if release_version <= lenient_semver::parse(APP_VERSION).unwrap() {
        info!("No new release found");
        return Ok(false);
    }

    if !ARGS.update_elevate_restart {
        if confirm_update(&release.version).await {
            debug!("Update confirmed");
        } else {
            debug!("Update ignored");
            return Ok(false);
        }
    }

    let current_exe = env::current_exe()
        .and_then(|p| p.canonicalize())
        .context("Failed to locate current executable")?;
    let needs_elevation = !faccess::PathExt::writable(current_exe.parent().unwrap());
    if needs_elevation {
        debug!("Elevation is required for update");
        if is_elevated::is_elevated() {
            debug!("Already running elevated");
        } else {
            debug!("Not currently elevated");
            debug!("Restarting app elevated");

            restart_elevated().context("Failed to restart with elevation")?;

            return Ok(true);
        }
    } else {
        debug!("Elevation is not required for update");
    }

    let asset = release
        .assets
        .into_iter()
        .find(|asset| {
            Path::new(&asset.name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        })
        .context("No release executable asset found")?;

    debug!(
        "Found release asset [{}] at {}",
        asset.name, asset.download_url
    );

    let tmp_dir = tempfile::Builder::new()
        .prefix(APP_NAME)
        .tempdir()
        .context("Failed to create temporary directory")?;
    let tmp_bin_path = tmp_dir.path().join(&asset.name);
    let tmp_bin = File::create(&tmp_bin_path)
        .await
        .context("Error creating temporary file")?
        .into_std()
        .await;

    tokio::task::spawn_blocking(move || download_file(&asset.download_url, tmp_bin))
        .await
        .context("Error downloading updated executable")?
        .context("Error downloading updated executable")?;

    debug!("Downloaded asset to {}", tmp_bin_path.display());

    let moved_bin = current_exe.with_extension("exe.bak");

    fs::rename(&current_exe, &moved_bin)
        .await
        .context("Failed to move current executable")?;
    match fs::rename(&tmp_bin_path, &current_exe).await {
        Ok(_) => {}
        Err(e) if e.raw_os_error() == Some(17) => {
            fs::copy(&tmp_bin_path, &current_exe)
                .await
                .context("Failed to copy updated executable to current executable path")?;
        }
        Err(e) => {
            return Err(e)
                .context("Failed to move updated executable to current executable path")?;
        }
    }

    debug!("Switched out binary");

    restart(&current_exe, &moved_bin).context("Failed to restart with updated executable")?;

    Ok(true)
}

fn restart(new_exe: &Path, old_exe: &Path) -> anyhow::Result<()> {
    let current_args = env::args().skip(1);

    std::process::Command::new(new_exe)
        .args(current_args)
        .arg("--update-old-bin")
        .arg(old_exe)
        .arg("--singleton-wait-for-shutdown")
        .stdin(Stdio::inherit())
        .spawn()
        .context("Error spawning process")?;

    Ok(())
}

fn restart_elevated() -> anyhow::Result<()> {
    let exe = U16CString::from_os_str(
        env::current_exe()
            .context("Failed to locate current executable")?
            .into_os_string(),
    )
    .context("Current executable has an invalid path?")?;
    let current_args = env::args().skip(1).collect::<Vec<_>>().join(" ");
    let new_args = "--update-elevate-restart --singleton-wait-for-shutdown";
    let args = U16CString::from_str(format!("{current_args} {new_args}"))
        .context("Arguments contain invalid characters")?;

    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            u16cstr!("runas").as_ptr(),
            exe.as_ptr(),
            args.as_ptr(),
            ptr::null_mut(),
            SW_SHOWDEFAULT,
        )
    };

    if result <= 32 as _ {
        return Err(io::Error::last_os_error()).context("Failed to run ShellExecuteW");
    }

    Ok(())
}

fn load_releases() -> Result<Vec<Release>, self_update::errors::Error> {
    self_update::backends::github::ReleaseList::configure()
        .repo_owner("OpenByteDev")
        .repo_name("burnt-sushi")
        .build()?
        .fetch()
}

async fn confirm_update(version: &str) -> bool {
    const POWERSHELL_APP_ID: &str =
        "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";
    const CONFIRM_ACTION: &str = "Update";
    const IGNORE_ACTION: &str = "Ignore";

    let (confirm_tx, mut confirm_rx) = tokio::sync::mpsc::channel::<bool>(1);

    let manager = ToastManager::new(POWERSHELL_APP_ID);
    let mut toast = Toast::new();
    toast
        .text1("BurntSushi")
        .text2(Text::new(format!("Update app to to {version}?")))
        .action(Action::new("Update", CONFIRM_ACTION, CONFIRM_ACTION))
        .action(Action::new("Ignore", IGNORE_ACTION, IGNORE_ACTION));

    let confirm_tx2 = confirm_tx.clone();
    let confirm_tx3 = confirm_tx.clone();
    let confirm_tx4 = confirm_tx.clone();

    let confirmed = manager.show_with_callbacks(
        &toast,
        Some(Box::new(move |res| {
            let confirmed = match res {
                Ok(arg) => {
                    debug!("Update toast activated (arg={arg})");
                    arg == CONFIRM_ACTION
                }
                Err(err) => {
                    debug!("Update toast activation failed (err={err})");
                    false
                }
            };
            confirm_tx2.try_send(confirmed).unwrap();
        })),
        Some(Box::new(move |res| {
            match res {
                Ok(reason) => debug!("Update toast dismissed (reason={reason:?})"),
                Err(err) => debug!("Update toast dismissal failed (err={err})"),
            }
            confirm_tx3.try_send(false).unwrap();
        })),
        Some(Box::new(move |err| {
            error!("Update toast failed: {err}");
            confirm_tx4.try_send(false).unwrap();
        })),
    );

    if let Err(err) = confirmed {
        error!("Failed to show toast: {err}");
        return false;
    }

    confirm_rx.recv().await.unwrap()
}

fn download_file(url: &str, target: impl Write) -> Result<(), self_update::errors::Error> {
    self_update::Download::from_url(url)
        .set_header(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("application/octet-stream"),
        )
        .download_to(target)
}
