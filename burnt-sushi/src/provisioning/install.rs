use std::env;

use anyhow::{Context, anyhow};

use crate::{DEFAULT_BLOCKER_FILE_NAME, resolver};

pub async fn install() -> anyhow::Result<()> {
    let current_location = env::current_exe().context("Failed to locate current executable")?;
    let install_dir = current_location
        .parent()
        .ok_or_else(|| anyhow!("Failed to determine parent directory"))?;
    let blocker_location = install_dir.join(DEFAULT_BLOCKER_FILE_NAME);

    resolver::install_blocker(&blocker_location)
        .await
        .context("Failed to write blocker to disk")?;

    Ok(())
}
