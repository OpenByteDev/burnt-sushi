fn main() {
    cargo_emit::rerun_if_changed!("schema\\spotify-ad-guard.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema\\spotify-ad-guard.capnp")
        .run()
        .unwrap();
}
