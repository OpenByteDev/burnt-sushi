use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_language(0x0409 /* English */);
    res.set_icon("icon.ico");
    res.set_icon_with_id("icon.ico", "TRAYICON");
    res.set_manifest_file("BurntSushi.exe.manifest");
    res.set("FileDescription", env!("CARGO_PKG_DESCRIPTION"));
    res.set("ProductName", "BurntSushi");
    res.set("OriginalFilename", "BurntSushi.exe");
    res.set("CompanyName", "OpenByte");
    res.compile().unwrap();

    fs::copy(
        build_crate(
            "burnt-sushi-blocker",
            "x86_64-pc-windows-msvc",
            "burnt_sushi_blocker.dll",
        ),
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("BurntSushiBlocker_x64.dll"),
    )
    .unwrap();

    let mut source_config_path = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    source_config_path.push("..");
    source_config_path.push("filter.toml");

    let mut target_config_path = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    target_config_path.push("filter.toml");
    fs::copy(source_config_path, target_config_path).unwrap();
}

fn build_crate(name: &str, target: &str, file: &str) -> PathBuf {
    // TODO: use encargo
    let cargo_exe = PathBuf::from(env::var_os("CARGO").unwrap());
    let is_release = env::var("PROFILE").unwrap().eq_ignore_ascii_case("release");
    let mut crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    crate_dir.push("..");
    crate_dir.push(name);

    let mut command = Command::new(cargo_exe);

    if cfg!(nightly) {
        command.arg("+nightly");
    }

    command
        .arg("build")
        .arg("--target")
        .arg(target)
        .current_dir(&crate_dir);

    if is_release {
        command.arg("--release");
    }

    let status = command.spawn().unwrap().wait().unwrap();
    assert!(status.success());

    let mut crate_artifact = crate_dir;
    crate_artifact.push("target");
    crate_artifact.push(target);
    crate_artifact.push(if is_release { "release" } else { "debug" });
    crate_artifact.push(file);

    assert!(crate_artifact.exists(), "{crate_artifact:?}");

    crate_artifact
}
