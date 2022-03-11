use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    fs::copy(
        build_inject_payload("i686-pc-windows-msvc"),
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("inject_payload_x86.dll"),
    )
    .unwrap();

    let mut source_config_path = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    source_config_path.push("..");
    source_config_path.push("filter.toml");

    let mut target_config_path = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    target_config_path.push("filter.toml");
    fs::copy(source_config_path, target_config_path).unwrap();
}

fn build_inject_payload(target: &str) -> PathBuf {
    let cargo_exe = PathBuf::from(env::var_os("CARGO").unwrap());
    let is_release = env::var("PROFILE").unwrap().eq_ignore_ascii_case("release");
    let mut payload_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    payload_dir.push("..");
    payload_dir.push("inject-payload");
    cargo_emit::rerun_if_changed!("inject-payload");
    cargo_emit::rerun_if_changed!("inject-payload\\src");
    cargo_emit::rerun_if_changed!("inject-payload\\src\\lib.rs");
    cargo_emit::rerun_if_changed!("inject-payload\\Cargo.toml");
    cargo_emit::rerun_if_changed!("inject-payload\\Cargo.lock");
    cargo_emit::rerun_if_changed!("inject-payload\\build.rs");

    let status = Command::new(&cargo_exe)
        .arg("build")
        .arg("--target")
        .arg(target)
        .current_dir(&payload_dir)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    assert!(status.success());

    let mut payload_dll = payload_dir;
    payload_dll.push("target");
    payload_dll.push(target);
    payload_dll.push(if is_release { "release" } else { "debug" });
    payload_dll.push("inject_payload.dll");

    assert!(payload_dll.exists());

    payload_dll
}
