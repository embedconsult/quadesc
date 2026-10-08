fn main() {
    println!("cargo:rerun-if-changed=examples/memory.x");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("arm")
        && std::env::var_os("CARGO_FEATURE_FIRMWARE").is_some()
    {
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        std::fs::copy("examples/memory.x", out.join("memory.x")).unwrap();
        println!("cargo:rustc-link-search={}", out.display());
        println!("cargo:rustc-link-arg-examples=-Tlink.x");
    }
}
