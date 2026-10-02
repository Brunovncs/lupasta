fn main() {
    // The window icon: GPUI loads icon resource 1 from the executable.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/lupasta.rc");
        println!("cargo:rerun-if-changed=assets/icons/icon.ico");
        embed_resource::compile("assets/lupasta.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
