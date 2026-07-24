use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::Context;
use log::{debug, error, info};
use reqwest::header::HeaderValue;
use self_update::update::Release;
use tokio::fs::{self, File};

use super::{msi, network};
use crate::{APP_AUTHOR, APP_NAME, APP_VERSION, toast};

const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

fn last_check_marker_path() -> Option<PathBuf> {
    let mut path = dirs::data_local_dir()?;
    path.push(APP_AUTHOR);
    path.push(APP_NAME);
    path.push(".last-update-check");
    Some(path)
}

async fn checked_recently(marker: &Path) -> bool {
    let Ok(metadata) = fs::metadata(marker).await else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    modified.elapsed().is_ok_and(|elapsed| elapsed < UPDATE_CHECK_INTERVAL)
}

async fn touch_check_marker(marker: &Path) {
    if let Some(parent) = marker.parent() {
        let _ = fs::create_dir_all(parent).await;
    }
    if let Err(e) = fs::write(marker, b"").await {
        error!("Failed to persist update check marker: {e}");
    }
}

pub async fn update() -> anyhow::Result<bool> {
    if !should_check_for_update().await {
        return Ok(false);
    }

    let Some(release) = find_latest_release().await? else {
        return Ok(false);
    };

    if ask_for_approval(&release.version).await {
        debug!("Update confirmed");
    } else {
        debug!("Update ignored");
        return Ok(false);
    }

    let via_msi = perform_update(release).await?;

    restart(&current_exe()?, via_msi).context("Failed to restart with updated executable")?;

    Ok(true)
}

fn current_exe() -> anyhow::Result<PathBuf> {
    env::current_exe()
        .and_then(|p| p.canonicalize())
        .context("Failed to locate current executable")
}

async fn should_check_for_update() -> bool {
    if network::is_metered_connection() {
        debug!("Skipping update check, connection is metered");
        return false;
    }

    let Some(marker) = last_check_marker_path() else {
        return true;
    };
    if checked_recently(&marker).await {
        debug!("Skipping update check, already checked within the last week");
        return false;
    }
    touch_check_marker(&marker).await;
    true
}

async fn find_latest_release() -> anyhow::Result<Option<Release>> {
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
        return Ok(None);
    }

    Ok(Some(release))
}

async fn perform_update(release: Release) -> anyhow::Result<bool> {
    let current_exe = current_exe()?;

    let via_msi = msi::is_installed();
    let target_extension = if via_msi { "msi" } else { "exe" };

    let asset = release
        .assets
        .into_iter()
        .find(|asset| {
            Path::new(&asset.name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(target_extension))
        })
        .context("No matching release asset found")?;

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

    if via_msi {
        update_via_msi(&tmp_bin_path, &current_exe).await?;
    } else {
        update_via_exe_swap(&tmp_bin_path, &current_exe).await?;
    }

    Ok(via_msi)
}

async fn update_via_msi(msi_path: &Path, current_exe: &Path) -> anyhow::Result<()> {
    let install_dir = current_exe
        .parent()
        .context("Failed to determine current install directory")?;

    msi::upgrade(msi_path, install_dir).await?;

    debug!("Installed update via msiexec");

    Ok(())
}

async fn update_via_exe_swap(tmp_bin_path: &Path, current_exe: &Path) -> anyhow::Result<()> {
    let moved_bin = current_exe.with_extension("exe.bak");

    fs::rename(current_exe, &moved_bin)
        .await
        .context("Failed to move current executable")?;
    match fs::rename(tmp_bin_path, current_exe).await {
        Ok(_) => {}
        Err(e) if e.raw_os_error() == Some(17) => {
            fs::copy(tmp_bin_path, current_exe)
                .await
                .context("Failed to copy updated executable to current executable path")?;
        }
        Err(e) => {
            return Err(e)
                .context("Failed to move updated executable to current executable path")?;
        }
    }

    debug!("Switched out binary");

    Ok(())
}

fn restart(new_exe: &Path, via_msi: bool) -> anyhow::Result<()> {
    let mut command = std::process::Command::new(new_exe);
    command.args(env::args().skip(1));

    if !via_msi {
        command.arg("--update-old-bin").arg(new_exe.with_extension("exe.bak"));
    }

    command
        .arg("--singleton-wait-for-shutdown")
        .stdin(Stdio::inherit())
        .spawn()
        .context("Error spawning process")?;

    Ok(())
}

fn load_releases() -> Result<Vec<Release>, self_update::errors::Error> {
    self_update::backends::github::ReleaseList::configure()
        .repo_owner("OpenByteDev")
        .repo_name("burnt-sushi")
        .build()?
        .fetch()
}

async fn ask_for_approval(version: &str) -> bool {
    toast::show(
        "BurntSushi",
        &format!("Update app to {version}?"),
        &[("Update", true), ("Ignore", false)],
    )
    .await
    .unwrap_or(false)
}

fn download_file(url: &str, target: impl Write) -> Result<(), self_update::errors::Error> {
    self_update::Download::from_url(url)
        .set_header(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("application/octet-stream"),
        )
        .download_to(target)
}
