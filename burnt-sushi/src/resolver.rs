use std::{
    env, io,
    path::{Path, PathBuf},
};

use log::{debug, error, warn};

use crate::{
    args::ARGS, blocker::FilterConfig, APP_AUTHOR, APP_NAME_WITH_VERSION,
    DEFAULT_BLOCKER_FILE_NAME, DEFAULT_FILTER_FILE_NAME,
};

pub async fn resolve_blocker() -> io::Result<PathBuf> {
    async fn try_load_blocker(
        path: &Path,
        check_len: bool,
        write_if_absent: bool,
    ) -> io::Result<()> {
        let payload_bytes = include_bytes!(concat!(env!("OUT_DIR"), "\\BurntSushiBlocker_x86.dll"));

        debug!("Looking for blocker at '{}'", path.display());
        if let Ok(metadata) = tokio::fs::metadata(path).await {
            if metadata.is_file() {
                debug!("Found blocker at '{}'", path.display());
                if check_len && metadata.len() != payload_bytes.len() as u64 {
                    debug!(
                        "Blocker at '{}' was ignored due to incorrect size.",
                        path.display()
                    );
                } else {
                    return Ok(());
                }
            }
        }
        if write_if_absent {
            debug!("Writing blocker to '{}'", path.display());
            tokio::fs::create_dir_all(path.parent().unwrap()).await?;
            tokio::fs::write(&path, payload_bytes).await?;
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Blocker not found at given path.",
            ))
        }
    }

    debug!("Looking for blocker according to cli args...");
    if let Some(config_path) = &ARGS.blocker {
        if try_load_blocker(config_path, false, true).await.is_ok() {
            return Ok(config_path.to_path_buf());
        } else {
            debug!("Looking for blocker according to cli args...");
        }
    }

    debug!("Looking for blocker next to executable...");
    if let Some(sibling_path) = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join(DEFAULT_BLOCKER_FILE_NAME)))
    {
        if try_load_blocker(&sibling_path, false, false).await.is_ok() {
            return Ok(sibling_path);
        }
    }

    debug!("Looking for existing blocker in temporary directory...");
    if let Some(temp_path) = env::temp_dir().parent().map(|p| {
        p.join(APP_AUTHOR)
            .join(APP_NAME_WITH_VERSION)
            .join(DEFAULT_BLOCKER_FILE_NAME)
    }) {
        if try_load_blocker(&temp_path, true, true).await.is_ok() {
            return Ok(temp_path);
        }
    }

    error!("Could not find or create blocker.");
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Could not find or create blocker.",
    ))
}

pub async fn resolve_filter_config() -> io::Result<FilterConfig> {
    async fn try_load_filter_config_from_path(
        path: Option<&Path>,
        write_if_absent: bool,
    ) -> io::Result<FilterConfig> {
        let default_filter_bytes = include_str!(concat!(env!("OUT_DIR"), "\\filter.toml"));

        if let Some(path) = path {
            debug!("Looking for filter config at '{}'", path.display());
            if let Ok(filters) = tokio::fs::read_to_string(path).await {
                debug!("Found filter config at '{}'", path.display());
                try_load_filter_config_from_str(&filters)
            } else if write_if_absent {
                debug!("Writing default filter config to '{}'", path.display());
                tokio::fs::create_dir_all(path.parent().unwrap()).await?;
                tokio::fs::write(&path, default_filter_bytes).await?;
                try_load_filter_config_from_str(default_filter_bytes)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Filter config did not exist.",
                ))
            }
        } else {
            debug!("Loading default filter config...");
            try_load_filter_config_from_str(default_filter_bytes)
        }
    }

    fn try_load_filter_config_from_str(filter_config: &str) -> io::Result<FilterConfig> {
        match toml::from_str(filter_config) {
            Ok(filter_config) => Ok(filter_config),
            Err(_) => {
                warn!("Failed to parse filter config.");
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Filter config is invalid.",
                ))
            }
        }
    }

    debug!("Looking for filter config according to cli args...");
    if let Some(config_path) = &ARGS.filters {
        if let Ok(filters) = try_load_filter_config_from_path(Some(config_path), true).await {
            return Ok(filters);
        }
    }

    debug!("Looking for filter config next to executable...");
    if let Some(sibling_path) = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join(DEFAULT_FILTER_FILE_NAME)))
    {
        if let Ok(filters) = try_load_filter_config_from_path(Some(&sibling_path), false).await {
            return Ok(filters);
        }
    }

    try_load_filter_config_from_path(None, false).await
}
