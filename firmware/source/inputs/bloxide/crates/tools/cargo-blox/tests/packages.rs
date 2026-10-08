// Copyright 2025 Bloxide, all rights reserved
use bloxide_codegen::packages;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;
fn put(path: impl AsRef<Path>, content: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
fn fixture() -> (TempDir, PathBuf, PathBuf) {
    let temp = TempDir::new().unwrap();
    let app = temp.path().join("app");
    let package = temp.path().join("package");
    put(app.join("Cargo.toml"), "[workspace]\nmembers = []\n");
    put(
        app.join("blox-workspace.toml"),
        "format = 1\n[packages.lamp]\npackage = 'lamp'\npath = '../package'\n",
    );
    put(package.join("blox-package.toml"), "format = 1\n[package]\nname = 'lamp'\nversion = '0.1.0'\nblox-schema = 1\n[[bloxes]]\nname = 'lamp'\npath = 'bloxes/lamp/blox.toml'\ncrate = 'lamp-blox'\n[[crates]]\nname = 'lamp-context'\npath = 'context/Cargo.toml'\n");
    put(
        package.join("bloxes/lamp/blox.toml"),
        "[actor]\nname = 'Lamp'\n",
    );
    put(
        package.join("context/Cargo.toml"),
        "[package]\nname = 'lamp-context'\nversion = '0.1.0'\n",
    );
    put(package.join("context/src/lib.rs"), "#![no_std]\n");
    (temp, app, package)
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
#[test]
fn imported_pure_toml_is_explicit_and_mutation_invalidates_lock() {
    let (_temp, app, package) = fixture();
    packages::resolve(&app).unwrap();
    packages::verify(&app).unwrap();
    let composition = packages::load(&app).unwrap();
    assert_eq!(composition.qualified["lamp::lamp"], "lamp-blox");
    assert_eq!(
        composition.bloxes["lamp-blox"].toml_path,
        package.join("bloxes/lamp/blox.toml")
    );
    put(
        package.join("context/src/lib.rs"),
        "#![no_std]\npub fn changed() {}\n",
    );
    assert!(packages::verify(&app)
        .unwrap_err()
        .to_string()
        .contains("blox.lock differs"));
}
#[test]
fn git_sources_check_full_revision_url_cleanliness_and_preserve_cargo_identity() {
    let (_temp, app, package) = fixture();
    git(&package, &["init", "-q"]);
    git(&package, &["add", "."]);
    git(
        &package,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@invalid",
            "commit",
            "-qm",
            "input",
        ],
    );
    git(
        &package,
        &["remote", "add", "origin", "https://example.invalid/lamp"],
    );
    let rev = git(&package, &["rev-parse", "HEAD"]);
    put(app.join("blox-workspace.toml"), &format!("format = 1\n[packages.lamp]\npackage = 'lamp'\ngit = 'https://example.invalid/lamp'\nrev = '{rev}'\ncheckout = '../package'\n"));
    packages::resolve(&app).unwrap();
    packages::verify(&app).unwrap();
    let composition = packages::load(&app).unwrap();
    assert_eq!(
        composition.crates["lamp-context"]["rev"].as_str(),
        Some(rev.as_str())
    );
    assert!(composition.crates["lamp-context"].get("path").is_none());
    put(package.join("context/src/lib.rs"), "// dirty\n");
    assert!(packages::load(&app)
        .err()
        .unwrap()
        .to_string()
        .contains("dirty"));
    let config = fs::read_to_string(app.join("blox-workspace.toml")).unwrap();
    put(
        app.join("blox-workspace.toml"),
        &(config + "\n[overrides.lamp]\npath = '../package'\n"),
    );
    packages::resolve(&app).unwrap();
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(app.join("blox.lock")).unwrap()).unwrap();
    assert_eq!(lock["packages"][0]["source"].as_str(), Some("path"));
    assert!(lock["packages"][0].get("revision").is_none());
    assert!(
        packages::load(&app).unwrap().patches["https://example.invalid/lamp"]
            .contains_key("lamp-context")
    );
}
#[test]
fn duplicate_names_and_escaped_exports_fail() {
    let (_temp, app, package) = fixture();
    let manifest = fs::read_to_string(package.join("blox-package.toml")).unwrap();
    put(
        package.join("blox-package.toml"),
        &(manifest.clone() + "\n[[crates]]\nname='lamp-context'\npath='context/Cargo.toml'\n"),
    );
    assert!(packages::load(&app)
        .err()
        .unwrap()
        .to_string()
        .contains("duplicate exported"));
    put(
        package.join("blox-package.toml"),
        &manifest.replace("context/Cargo.toml", "../app/Cargo.toml"),
    );
    assert!(packages::load(&app)
        .err()
        .unwrap()
        .to_string()
        .contains("escapes root"));
}
#[test]
fn incomplete_composition_is_an_error() {
    let (_temp, app, package) = fixture();
    let manifest = fs::read_to_string(package.join("blox-package.toml")).unwrap();
    put(package.join("blox-package.toml"), &(manifest+"\n[dependencies.missing]\npackage='absent'\ngit='https://example.invalid/missing'\nrev='1111111111111111111111111111111111111111'\n"));
    assert!(packages::load(&app)
        .err()
        .unwrap()
        .to_string()
        .contains("requires explicitly selected"));
}

#[test]
fn conflicting_revisions_and_package_cycles_are_rejected() {
    let (_temp, app, package) = fixture();
    let other = package.parent().unwrap().join("other");
    let revision = "1111111111111111111111111111111111111111";
    put(other.join("blox-package.toml"), &format!("format=1\n[package]\nname='other'\nversion='0.1.0'\nblox-schema=1\n[dependencies.lamp]\npackage='lamp'\ngit='https://example.invalid/lamp'\nrev='{revision}'\n"));
    let manifest = fs::read_to_string(package.join("blox-package.toml")).unwrap();
    put(package.join("blox-package.toml"), &(manifest + &format!("\n[dependencies.other]\npackage='other'\ngit='https://example.invalid/other'\nrev='{revision}'\n")));
    let composition = format!("format=1\n[packages.lamp]\npackage='lamp'\ngit='https://example.invalid/lamp'\nrev='{revision}'\ncheckout='../package'\n[packages.other]\npackage='other'\ngit='https://example.invalid/other'\nrev='{revision}'\ncheckout='../other'\n[overrides.lamp]\npath='../package'\n[overrides.other]\npath='../other'\n");
    put(app.join("blox-workspace.toml"), &composition);
    assert!(packages::load(&app)
        .err()
        .unwrap()
        .to_string()
        .contains("dependency cycle"));
    let changed = fs::read_to_string(other.join("blox-package.toml"))
        .unwrap()
        .replace(revision, "2222222222222222222222222222222222222222");
    put(other.join("blox-package.toml"), &changed);
    assert!(packages::load(&app)
        .err()
        .unwrap()
        .to_string()
        .contains("conflicting source revision"));
}

fn commit_package(package: &Path) -> String {
    git(package, &["init", "-q"]);
    git(package, &["add", "."]);
    git(
        package,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@invalid",
            "commit",
            "-qm",
            "source",
        ],
    );
    git(package, &["rev-parse", "HEAD"])
}

#[test]
fn ignored_sources_are_snapshotted_and_cannot_claim_git_provenance() {
    let (_temp, app, package) = fixture();
    put(package.join(".gitignore"), "bloxes/\nignored.rs\n");
    let rev = commit_package(&package);
    git(
        &package,
        &["remote", "add", "origin", "https://example.invalid/lamp"],
    );
    put(app.join("blox-workspace.toml"), &format!("format=1\n[packages.lamp]\npackage='lamp'\ngit='https://example.invalid/lamp'\nrev='{rev}'\ncheckout='../package'\n"));
    assert!(packages::resolve(&app)
        .unwrap_err()
        .to_string()
        .contains("not in pinned tree"));
    put(
        app.join("blox-workspace.toml"),
        "format=1\n[packages.lamp]\npackage='lamp'\npath='../package'\n",
    );
    put(
        package.join("context/src/lib.rs"),
        "include!(\"../../ignored.rs\");\n",
    );
    put(package.join("ignored.rs"), "pub const LEVEL: u32 = 1;\n");
    packages::resolve(&app).unwrap();
    let lock = fs::read(app.join("blox.lock")).unwrap();
    let parsed: toml::Value = toml::from_str(std::str::from_utf8(&lock).unwrap()).unwrap();
    assert_eq!(parsed["packages"][0]["source"].as_str(), Some("path"));
    assert_eq!(parsed["packages"][0]["dirty"].as_bool(), Some(true));
    packages::verify(&app).unwrap();
    put(package.join("ignored.rs"), "pub const LEVEL: u32 = 2;\n");
    assert!(packages::verify(&app).is_err());
    assert_eq!(fs::read(app.join("blox.lock")).unwrap(), lock);
}

#[test]
fn snapshot_exclusions_are_precise_and_regeneration_is_stable() {
    let (_temp, app, package) = fixture();
    commit_package(&package);
    packages::resolve(&app).unwrap();
    let before = fs::read(app.join("blox.lock")).unwrap();
    // Ordinary same-named files are still inputs.
    put(package.join("context/blox.lock"), "input");
    assert!(packages::verify(&app).is_err());
    fs::remove_file(package.join("context/blox.lock")).unwrap();
    put(package.join("target/build-output"), "disposable");
    // Git status must ignore true outputs, independently of source hashing.
    put(package.join(".gitignore"), "target/\n");
    git(&package, &["add", ".gitignore"]);
    git(
        &package,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@invalid",
            "commit",
            "-qm",
            "output rule",
        ],
    );
    packages::resolve(&app).unwrap();
    let after = fs::read(app.join("blox.lock")).unwrap();
    assert_ne!(before, after);
    put(package.join("target/build-output"), "changed output");
    packages::verify(&app).unwrap();
    packages::resolve(&app).unwrap();
    assert_eq!(after, fs::read(app.join("blox.lock")).unwrap());
}

#[cfg(unix)]
#[test]
fn symlink_submodule_and_output_exports_are_rejected() {
    let (_temp, app, package) = fixture();
    std::os::unix::fs::symlink(package.join("context/src/lib.rs"), package.join("alias.rs"))
        .unwrap();
    assert!(packages::resolve(&app)
        .unwrap_err()
        .to_string()
        .contains("symlink"));
    fs::remove_file(package.join("alias.rs")).unwrap();
    put(package.join(".gitmodules"), "");
    assert!(packages::resolve(&app)
        .unwrap_err()
        .to_string()
        .contains("submodules"));
    fs::remove_file(package.join(".gitmodules")).unwrap();
    let manifest = fs::read_to_string(package.join("blox-package.toml")).unwrap();
    put(
        package.join("target/lamp/blox.toml"),
        "[actor]\nname='Lamp'\n",
    );
    put(
        package.join("blox-package.toml"),
        &manifest.replace("bloxes/lamp/blox.toml", "target/lamp/blox.toml"),
    );
    assert!(packages::resolve(&app)
        .unwrap_err()
        .to_string()
        .contains("reserved output"));
}

#[test]
fn explicit_normal_dev_and_renamed_dependencies_share_access_ledger() {
    let (_temp, app, package) = fixture();
    let other = package.parent().unwrap().join("other");
    put(other.join("blox-package.toml"), "format=1\n[package]\nname='other'\nversion='0.1.0'\nblox-schema=1\n[[crates]]\nname='secret'\npath='Cargo.toml'\n");
    put(
        other.join("Cargo.toml"),
        "[package]\nname='secret'\nversion='0.1.0'\n",
    );
    let rev = "1111111111111111111111111111111111111111";
    put(app.join("blox-workspace.toml"), &format!("format=1\n[packages.lamp]\npackage='lamp'\npath='../package'\n[packages.other]\npackage='other'\ngit='https://example.invalid/other'\nrev='{rev}'\n[overrides.other]\npath='../other'\n"));
    put(app.join("Cargo.toml"), "[workspace]\nmembers=[]\n[workspace.dependencies]\nalias={package='secret', path='../other'}\nserde='1'\n");
    for kind in ["dependencies", "dev-dependencies"] {
        for name in ["secret", "alias"] {
            put(
                package.join("bloxes/lamp/blox.toml"),
                &format!("[actor]\nname='Lamp'\n[package.{kind}]\n{name}={{}}\n"),
            );
            assert!(packages::resolve(&app)
                .unwrap_err()
                .to_string()
                .contains("without a declared bundle dependency"));
        }
    }
    let manifest = fs::read_to_string(package.join("blox-package.toml")).unwrap();
    put(package.join("blox-package.toml"), &(manifest + &format!("\n[dependencies.other]\npackage='other'\ngit='https://example.invalid/other'\nrev='{rev}'\n")));
    packages::resolve(&app).unwrap();
    put(
        package.join("bloxes/lamp/blox.toml"),
        "[actor]\nname='Lamp'\n[package.dependencies]\nserde={}\n",
    );
    packages::resolve(&app).unwrap();
}

#[test]
fn exported_cargo_target_cannot_escape_package_snapshot() {
    let (_temp, app, package) = fixture();
    put(app.join("external.rs"), "pub fn outside() {}\n");
    let manifest = fs::read_to_string(package.join("context/Cargo.toml")).unwrap();
    put(
        package.join("context/Cargo.toml"),
        &(manifest + "\n[lib]\npath='../../app/external.rs'\n"),
    );
    assert!(packages::resolve(&app)
        .unwrap_err()
        .to_string()
        .contains("escapes package"));
}

#[test]
fn clean_git_subdirectory_path_is_attributed_clean() {
    let (_temp, app, package) = fixture();
    let root = package.parent().unwrap();
    commit_package(root);
    packages::resolve(&app).unwrap();
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(app.join("blox.lock")).unwrap()).unwrap();
    assert_eq!(lock["packages"][0]["dirty"].as_bool(), Some(false));
    assert_eq!(lock["packages"][0]["source"].as_str(), Some("path"));
}

#[test]
fn literal_rust_inputs_cannot_escape_or_use_reserved_outputs() {
    let (_temp, app, package) = fixture();
    for source in [
        "include!(\"../../../app/outside.rs\");",
        "const X: &str = include_str!(\"../../../app/outside.txt\");",
        "const X: &[u8] = include_bytes!(\"../../../app/outside.bin\");",
        "#[path=\"../../../app/outside.rs\"] mod outside;",
        "include!(\"../../target/ignored.rs\");",
    ] {
        put(package.join("context/src/lib.rs"), source);
        let error = packages::resolve(&app).unwrap_err().to_string();
        assert!(error.contains("literal Rust input"), "{error}");
    }
    put(
        package.join("context/src/lib.rs"),
        "// include!(\"../../../app/comment.rs\");\nconst TEXT: &str = \"include!(bad)\";\n",
    );
    packages::resolve(&app).unwrap();
}
