use std::path::Path;

use anyhow::Context;
use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS, System::ApplicationInstallationAndServicing::MsiEnumRelatedProductsW,
};

/// `UpgradeCode` from `wix/main.wxs`, constant across all versions of the MSI.
const UPGRADE_CODE: &str = "31d49aef-51d9-4e4e-ac9e-bb0ebfdebca1";

/// Whether an MSI-installed copy of the app (any version) is currently registered
/// with Windows Installer, as opposed to running as a portable exe.
pub fn is_installed() -> bool {
    let upgrade_code = widestring::U16CString::from_str(format!("{{{UPGRADE_CODE}}}")).unwrap();
    let mut product_buf = [0u16; 39];
    let result =
        unsafe { MsiEnumRelatedProductsW(upgrade_code.as_ptr(), 0, 0, product_buf.as_mut_ptr()) };
    result == ERROR_SUCCESS
}

pub async fn upgrade(msi_path: &Path, install_dir: &Path) -> anyhow::Result<()> {
    let status = tokio::process::Command::new("msiexec")
        .arg("/i")
        .arg(msi_path)
        .arg(format!("APPLICATIONFOLDER={}", install_dir.display()))
        .arg("/quiet")
        .arg("/norestart")
        .status()
        .await
        .context("Failed to run msiexec")?;

    if !status.success() {
        anyhow::bail!("msiexec exited with {status}");
    }

    Ok(())
}
