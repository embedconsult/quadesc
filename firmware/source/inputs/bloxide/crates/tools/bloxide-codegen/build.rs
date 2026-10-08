// Copyright 2025 Bloxide, all rights reserved
//! Capture the compiled generator identity, never infer it from mutable source
//! files when a previously built executable runs.
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().into())
}
fn files(path: &Path, output: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() {
            files(&entry.unwrap().path(), output);
        }
    } else {
        output.push(path.to_path_buf());
    }
}
fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut inputs = Vec::new();
    for path in [
        "src",
        "build.rs",
        "Cargo.toml",
        "../cargo-blox/src",
        "../cargo-blox/Cargo.toml",
    ] {
        files(&root.join(path), &mut inputs);
    }
    inputs.sort();
    let mut hash = Sha256::new();
    for path in inputs {
        hash.update(
            path.strip_prefix(&root)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes(),
        );
        hash.update([0]);
        hash.update(std::fs::read(path).unwrap());
    }
    println!(
        "cargo:rustc-env=BLOXIDE_GENERATOR_SHA256={:x}",
        hash.finalize()
    );
    let revision = git(&root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "source-archive".into());
    println!("cargo:rustc-env=BLOXIDE_GENERATOR_REVISION={revision}");
    for reference in [
        Some("HEAD".into()),
        git(&root, &["symbolic-ref", "-q", "HEAD"]),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(path) = git(&root, &["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
}
