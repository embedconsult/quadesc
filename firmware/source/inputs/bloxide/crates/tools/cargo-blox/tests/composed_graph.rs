// Copyright 2025 Bloxide, all rights reserved
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;
fn put(root: &Path, file: &str, content: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-blox"))
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("CARGO_NET_OFFLINE", "true")
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn selected_features_and_target_conditions_control_the_checked_graph() {
    let dir = TempDir::new().unwrap();
    let app = dir.path().join("app");
    let pkg = dir.path().join("package");
    put(&pkg, "blox-package.toml", "format=1\n[package]\nname='bundle'\nversion='0.1.0'\nblox-schema=1\n[[crates]]\nname='public-message'\npath='first/Cargo.toml'\n");
    for (path, version) in [("first", "0.1.0"), ("second", "0.2.0")] {
        put(
            &pkg,
            &format!("{path}/Cargo.toml"),
            &format!("[package]\nname='public-message'\nversion='{version}'\nedition='2021'\n"),
        );
        put(&pkg, &format!("{path}/src/lib.rs"), "pub struct Message;\n");
    }
    put(
        &app,
        "blox-workspace.toml",
        "format=1\n[packages.bundle]\npackage='bundle'\npath='../package'\n",
    );
    put(&app, "src/lib.rs", "pub fn application() {}\n");
    let base = "[package]\nname='consumer'\nversion='0.1.0'\nedition='2021'\n[workspace]\n";
    put(&app, "Cargo.toml", &(base.to_owned()+"[dependencies]\npublic-message={path='../package/first'}\nsecond={package='public-message',path='../package/second',optional=true}\n"));
    assert!(run(&app, &["blox", "resolve"]).status.success());
    assert!(run(&app, &["blox", "lock"]).status.success());
    let normal = run(&app, &["blox", "check", "--", "--locked"]);
    assert!(
        normal.status.success(),
        "{}",
        String::from_utf8_lossy(&normal.stderr)
    );
    let duplicate = run(
        &app,
        &["blox", "check", "--features", "second", "--", "--locked"],
    );
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("multiple sources"));
    // Target-inactive identity must not contaminate the selected host graph.
    put(&app, "Cargo.toml", &(base.to_owned()+"[target.'cfg(unix)'.dependencies]\npublic-message={path='../package/first'}\n[target.'cfg(not(unix))'.dependencies]\nsecond={package='public-message',path='../package/second'}\n"));
    assert!(run(&app, &["blox", "lock"]).status.success());
    let conditional = run(&app, &["blox", "check", "--", "--locked"]);
    assert!(
        conditional.status.success(),
        "{}",
        String::from_utf8_lossy(&conditional.stderr)
    );
    // A single incorrect path identity is still rejected.
    put(
        &app,
        "Cargo.toml",
        &(base.to_owned() + "[dependencies]\npublic-message={path='../package/second'}\n"),
    );
    assert!(run(&app, &["blox", "lock"]).status.success());
    let mismatch = run(&app, &["blox", "check", "--", "--locked"]);
    assert!(!mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("source mismatch"));
}

#[test]
fn multiple_workspace_roots_are_validated_before_compilation() {
    let dir = TempDir::new().unwrap();
    let app = dir.path().join("app");
    let pkg = dir.path().join("package");
    put(&pkg, "blox-package.toml", "format=1\n[package]\nname='bundle'\nversion='0.1.0'\nblox-schema=1\n[[crates]]\nname='public-message'\npath='first/Cargo.toml'\n");
    for (path, version) in [("first", "0.1.0"), ("second", "0.2.0")] {
        put(
            &pkg,
            &format!("{path}/Cargo.toml"),
            &format!("[package]\nname='public-message'\nversion='{version}'\nedition='2021'\n"),
        );
        put(&pkg, &format!("{path}/src/lib.rs"), "pub struct Message;\n");
    }
    put(
        &app,
        "Cargo.toml",
        "[workspace]\nmembers=['a','b']\nresolver='2'\n",
    );
    put(
        &app,
        "blox-workspace.toml",
        "format=1\n[packages.bundle]\npackage='bundle'\npath='../package'\n",
    );
    for name in ["a", "b"] {
        put(&app, &format!("{name}/Cargo.toml"), &format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\n[dependencies]\npublic-message={{path='../../package/first'}}\n"));
        put(&app, &format!("{name}/src/lib.rs"), "pub fn app() {}\n");
    }
    assert!(run(&app, &["blox", "resolve"]).status.success());
    assert!(run(&app, &["blox", "lock"]).status.success());
    let valid = run(&app, &["blox", "check", "--", "--locked"]);
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    let manifest = fs::read_to_string(app.join("b/Cargo.toml")).unwrap();
    put(
        &app,
        "b/Cargo.toml",
        &(manifest + "second={package='public-message',path='../../package/second'}\n"),
    );
    assert!(run(&app, &["blox", "lock"]).status.success());
    let invalid = run(&app, &["blox", "check", "--", "--locked"]);
    let stderr = String::from_utf8_lossy(&invalid.stderr);
    assert!(
        !invalid.status.success() && stderr.contains("multiple sources"),
        "{stderr}"
    );
    assert!(!stderr.contains("Checking "));
}

#[test]
fn named_test_target_includes_dev_dependency_identities() {
    let dir = TempDir::new().unwrap();
    let app = dir.path().join("app");
    let pkg = dir.path().join("package");
    put(&pkg, "blox-package.toml", "format=1\n[package]\nname='bundle'\nversion='0.1.0'\nblox-schema=1\n[[crates]]\nname='public-message'\npath='first/Cargo.toml'\n");
    for (path, version) in [("first", "0.1.0"), ("second", "0.2.0")] {
        put(
            &pkg,
            &format!("{path}/Cargo.toml"),
            &format!("[package]\nname='public-message'\nversion='{version}'\nedition='2021'\n"),
        );
        put(&pkg, &format!("{path}/src/lib.rs"), "pub struct Message;\n");
    }
    put(&app, "Cargo.toml", "[package]\nname='consumer'\nversion='0.1.0'\nedition='2021'\n[workspace]\n[dependencies]\npublic-message={path='../package/first'}\n[dev-dependencies]\nsecond={package='public-message',path='../package/second'}\n");
    put(&app, "src/lib.rs", "pub fn app() {}\n");
    put(&app, "tests/probe.rs", "#[test] fn probe() {}\n");
    put(
        &app,
        "blox-workspace.toml",
        "format=1\n[packages.bundle]\npackage='bundle'\npath='../package'\n",
    );
    assert!(run(&app, &["blox", "resolve"]).status.success());
    assert!(run(&app, &["blox", "lock"]).status.success());
    for flag in ["--test=probe", "--all-targets"] {
        let result = run(&app, &["blox", "check", "--", "--locked", flag]);
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            !result.status.success() && stderr.contains("multiple sources"),
            "{stderr}"
        );
        assert!(!stderr.contains("Checking "));
    }
}
