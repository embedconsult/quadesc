// Copyright 2025 Bloxide, all rights reserved
use std::{fs, process::Command};
use tempfile::TempDir;

#[test]
fn locked_generation_rejects_missing_malformed_stale_root_locks_without_mutation() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn input() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='lock-probe'\nversion='0.1.0'\nedition='2021'\n[workspace]\n",
    )
    .unwrap();
    let generate = |flag: &str| {
        Command::new(env!("CARGO_BIN_EXE_cargo-blox"))
            .current_dir(root)
            .args(["blox", "generate", flag])
            .output()
            .unwrap()
    };
    assert!(!generate("--locked").status.success());
    assert!(!root.join("Cargo.lock").exists());
    fs::write(root.join("Cargo.lock"), "NOT TOML\n").unwrap();
    assert!(!generate("--locked").status.success());
    assert_eq!(
        fs::read_to_string(root.join("Cargo.lock")).unwrap(),
        "NOT TOML\n"
    );
    fs::remove_file(root.join("Cargo.lock")).unwrap();
    let status = Command::new("cargo")
        .current_dir(root)
        .args(["generate-lockfile", "--offline"])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(generate("--frozen").status.success());
    let lock = fs::read_to_string(root.join("Cargo.lock"))
        .unwrap()
        .replace("0.1.0", "0.0.9");
    fs::write(root.join("Cargo.lock"), &lock).unwrap();
    assert!(!generate("--locked").status.success());
    assert_eq!(fs::read_to_string(root.join("Cargo.lock")).unwrap(), lock);
    assert_eq!(
        fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        "pub fn input() {}\n"
    );
}
