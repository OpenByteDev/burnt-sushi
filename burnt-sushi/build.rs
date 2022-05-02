use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    embed_resource::compile("resources.rc");

    fs::copy(
        build_crate(
            "burnt-sushi-blocker",
            "i686-pc-windows-msvc",
            "burnt_sushi_blocker.dll",
        ),
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("BurntSushiBlocker_x86.dll"),
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
    let cargo_exe = PathBuf::from(env::var_os("CARGO").unwrap());
    let is_release = env::var("PROFILE").unwrap().eq_ignore_ascii_case("release");
    let mut crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    crate_dir.push("..");
    crate_dir.push(name);
    cargo_emit::rerun_if_changed!("..\\{}", name);

    let status = Command::new(&cargo_exe)
        .arg("build")
        .arg("--target")
        .arg(target)
        .current_dir(&crate_dir)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    assert!(status.success());

    let mut crate_artifact = crate_dir;
    crate_artifact.push("target");
    crate_artifact.push(target);
    crate_artifact.push(if is_release { "release" } else { "debug" });
    crate_artifact.push(file);

    assert!(crate_artifact.exists(), "{:?}", crate_artifact);

    crate_artifact
}
