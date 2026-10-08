// Copyright 2025 Bloxide, all rights reserved
//! Generate code from all blox.toml files in the workspace.

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::utils::{find_workspace_root_from, DISCOVERY_MAX_DEPTH};

pub fn generate(workspace: Option<PathBuf>) -> anyhow::Result<()> {
    let root = workspace.unwrap_or_else(|| {
        let manifest_dir =
            PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()));
        find_workspace_root_from(&manifest_dir).unwrap_or(manifest_dir)
    });

    bloxide_codegen::packages::verify(&root)?;
    // Lint before generating (issues #114/#122): invalid TOML fails fast
    // with friendly diagnostics instead of codegen errors or Rust compile
    // errors downstream.
    let previous_dir = std::env::current_dir()?;
    std::env::set_current_dir(&root)?;
    let lint_result = crate::lint::lint();
    std::env::set_current_dir(previous_dir)?;
    lint_result?;

    // ── Blox crates ──────────────────────────────────────────────────────
    // Two source kinds (see bloxide_codegen::util::BloxSourceKind):
    //
    // - PureToml (blox.toml under a `bloxes/` directory): materialize a
    //   complete crate into target/bloxide-generated/crates/<crate-name>
    //   (Cargo.toml, build.rs, src/lib.rs, src/generated/, tests/).
    // - InCrate (stdlib crates like bloxide-timer/bloxide-supervisor with a
    //   hand-written Cargo.toml): regenerate src/generated/ in-crate.
    let discovered = bloxide_codegen::util::discover_bloxes(&root)?;
    let mut count = 0;
    let mut pure_toml_count = 0usize;
    for (crate_name, blox) in &discovered {
        match blox.kind {
            bloxide_codegen::util::BloxSourceKind::PureToml => {
                let crate_dir = root
                    .join(bloxide_codegen::blox_crate::GENERATED_WORKSPACE_DIR)
                    .join("crates")
                    .join(crate_name);
                bloxide_codegen::blox_crate::sync_blox_crate(&blox.toml_path, &root, &crate_dir)
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "failed to materialize blox crate {} from {}: {}",
                            crate_name,
                            blox.toml_path.display(),
                            e
                        )
                    })?;
                pure_toml_count += 1;
                count += 1;
            }
            bloxide_codegen::util::BloxSourceKind::InCrate => {
                // Imported Cargo crates own their build scripts. Never modify
                // another package checkout while generating the consumer.
                if !blox
                    .toml_path
                    .canonicalize()?
                    .starts_with(root.canonicalize()?)
                {
                    continue;
                }
                let toml_path = &blox.toml_path;
                let crate_dir = toml_path.parent().unwrap();
                let src_dir = crate_dir.join("src");
                let generated_dir = src_dir.join("generated");

                let files = match bloxide_codegen::generate_from_toml(toml_path) {
                    Ok(files) => files,
                    Err(e) => {
                        return Err(
                            e.context(format!("generation failed for {}", toml_path.display()))
                        )
                    }
                };

                // Nothing to emit for this crate (e.g. bloxide-core, whose
                // `[mailboxes]`-only blox.toml is consumed by its build.rs at build
                // time). Skip entirely: do not create src/generated/ or touch lib.rs.
                if files.is_empty() {
                    continue;
                }

                std::fs::create_dir_all(&generated_dir)?;

                for (filename, content) in &files {
                    // mod.rs is owned by `ensure_generated_mod` below (single writer —
                    // two writers with different formatting would ping-pong the file).
                    if filename == "mod.rs" {
                        continue;
                    }
                    let path = generated_dir.join(filename);

                    // Format the candidate before comparing so previously written
                    // (already rustfmt'd) files compare equal — otherwise every run
                    // rewrites every file (mtime churn) and spams "generated ...".
                    let content = rustfmt_text(content);

                    // Only write if changed (preserves mtime for caching)
                    let needs_write = if path.exists() {
                        std::fs::read_to_string(&path)? != content
                    } else {
                        true
                    };

                    if needs_write {
                        std::fs::write(&path, &content)?;
                        println!("bloxide: generated {}", path.display());
                    }
                }

                // Ensure lib.rs includes the generated module
                ensure_generated_mod(&src_dir, &files)?;
                count += 1;
            }
        }
    }

    // The generated workspace root manifest (target/bloxide-generated/Cargo.toml).
    if pure_toml_count > 0 {
        bloxide_codegen::blox_crate::sync_generated_workspace(&root)?;
    }

    println!("bloxide: processed {} blox.toml files", count);

    // ── Process system.toml files (example wiring) ───────────────────────
    // After generating blox crate code, materialize a complete example crate
    // per system.toml into target/bloxide-generated/examples/<crate-name>/
    // (Cargo.toml, build.rs, src/main.rs, src/lib.rs, src/generated/, tests/).
    // `cargo blox build`/`check`/`test`/`run` automatically regenerate example
    // wiring alongside blox crate code — no separate `cargo blox wire` step
    // needed.
    let mut wire_count = 0;
    for entry in WalkDir::new(&root)
        .max_depth(DISCOVERY_MAX_DEPTH)
        .into_iter()
        .filter_entry(|e| e.file_name() != "target")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "system.toml")
    {
        let system_path = entry.path();
        match bloxide_codegen::example_crate::sync_example(system_path, &root) {
            Ok(crate_name) => {
                println!(
                    "bloxide: materialized example crate {} (from {})",
                    crate_name,
                    system_path.display()
                );
            }
            Err(e) => {
                return Err(e.context(format!("failed to materialize {}", system_path.display())))
            }
        }
        wire_count += 1;
    }

    if wire_count > 0 {
        // Examples are members of the generated workspace too.
        bloxide_codegen::blox_crate::sync_generated_workspace(&root)?;
        println!("bloxide: processed {} system.toml files", wire_count);
    }

    // Generated files were rustfmt'd individually before writing (see
    // `rustfmt_text`) — no whole-workspace `cargo fmt` here, which would also
    // rewrite hand-written files.

    // ── IDE integration ──────────────────────────────────────────────────
    // rust-analyzer only auto-discovers the root Cargo.toml; the generated
    // workspace under target/ must be declared via the client-side
    // `rust-analyzer.linkedProjects` setting. (A project-local
    // rust-analyzer.toml cannot express it: linkedProjects is a *global*
    // config key, and rust-analyzer.toml files in workspace roots are only
    // parsed for workspace/local keys.) Emit .vscode/settings.json so a
    // fresh clone needs only `cargo blox generate` before the IDE indexes
    // both workspaces.
    sync_vscode_settings(&root)?;
    let source_lock = root.join("locks/blox-generated.Cargo.lock");
    if source_lock.exists() {
        std::fs::copy(
            source_lock,
            root.join(bloxide_codegen::blox_crate::GENERATED_WORKSPACE_DIR)
                .join("Cargo.lock"),
        )?;
    }
    generation_manifest(&root)?;
    Ok(())
}

/// Workspace-relative path of the IDE settings file emitted by `generate`.
const VSCODE_SETTINGS_PATH: &str = ".vscode/settings.json";

/// Write `.vscode/settings.json` declaring `rust-analyzer.linkedProjects` for
/// the root manifest and the generated workspace manifest.
///
/// Setting `linkedProjects` disables project auto-discovery, so the root
/// Cargo.toml must be listed explicitly alongside
/// target/bloxide-generated/Cargo.toml. Existing unrelated settings are
/// preserved (the file is gitignored, but a user may keep personal settings
/// in it); like all codegen output the file is only written when the merged
/// content changes.
fn sync_vscode_settings(root: &Path) -> anyhow::Result<()> {
    // Only meaningful once the generated workspace exists.
    let generated_manifest = root
        .join(bloxide_codegen::blox_crate::GENERATED_WORKSPACE_DIR)
        .join("Cargo.toml");
    if !generated_manifest.exists() {
        return Ok(());
    }

    let settings_path = root.join(VSCODE_SETTINGS_PATH);
    let mut settings: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(serde_json::Value::Object(map)) => settings = map,
            _ => {
                // e.g. JSONC comments, which serde_json cannot parse — leave
                // the user's file untouched rather than clobbering it.
                eprintln!(
                    "bloxide: warning: {} is not plain JSON; skipping \
                     rust-analyzer.linkedProjects emission",
                    settings_path.display()
                );
                return Ok(());
            }
        }
    }

    settings.insert(
        "rust-analyzer.linkedProjects".to_string(),
        serde_json::json!(["./Cargo.toml", "./target/bloxide-generated/Cargo.toml"]),
    );
    let content = serde_json::to_string_pretty(&serde_json::Value::Object(settings))? + "\n";

    // Only write if changed (preserves mtime for caching)
    if settings_path.exists() && std::fs::read_to_string(&settings_path)? == content {
        return Ok(());
    }
    std::fs::create_dir_all(settings_path.parent().unwrap())?;
    std::fs::write(&settings_path, content)?;
    println!("bloxide: generated {}", settings_path.display());
    Ok(())
}

/// Format Rust source via `rustfmt` (edition 2021). Best effort: returns the
/// input unchanged if rustfmt is unavailable or fails — the file is still
/// written, just unformatted.
fn rustfmt_text(source: &str) -> String {
    use std::io::Write;
    let mut child = match std::process::Command::new("rustfmt")
        .args(["--edition", "2021"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return source.to_string(),
    };
    if child
        .stdin
        .take()
        .map(|mut s| s.write_all(source.as_bytes()))
        .is_none()
    {
        return source.to_string();
    }
    match child.wait_with_output() {
        Ok(out) if out.status.success() => {
            String::from_utf8(out.stdout).unwrap_or_else(|_| source.to_string())
        }
        _ => source.to_string(),
    }
}

fn ensure_generated_mod(
    src_dir: &Path,
    generated_files: &[(String, String)],
) -> anyhow::Result<()> {
    let lib_rs = src_dir.join("lib.rs");
    let mod_line = "pub mod generated;\n";

    if lib_rs.exists() {
        let content = std::fs::read_to_string(&lib_rs)?;
        if !content.contains("mod generated") {
            anyhow::bail!(
                "{} must declare {mod_line} before generation",
                lib_rs.display()
            );
        }
    }

    // Write generated/mod.rs with pub mod declarations for each generated file.
    // Only include files in the generated_files list — stale files left on disk
    // from previous generations are NOT preserved, so removing a blox from the
    // manifest correctly removes its module from mod.rs.
    let generated_mod = src_dir.join("generated").join("mod.rs");
    let mod_names: Vec<String> = generated_files
        .iter()
        .filter_map(|(filename, _)| {
            if filename.ends_with(".rs") && filename != "mod.rs" {
                Some(filename.trim_end_matches(".rs").to_string())
            } else {
                None
            }
        })
        .collect();

    let mut mod_content = String::from(
        "// Copyright 2025 Bloxide, all rights reserved\n// Auto-generated module.\n// Files in this directory are generated by bloxide-codegen.\n//! Generated by bloxide-codegen from blox.toml — do not edit by hand; regenerate with `cargo blox generate`.\n",
    );
    for mod_name in mod_names {
        // topology module exports a #[macro_export] handler_table macro
        if mod_name == "topology" {
            mod_content.push_str("#[macro_use]\n");
        }
        mod_content.push_str(&format!("pub mod {};\n", mod_name));
        mod_content.push_str(&format!(
            "#[allow(unused_imports)]\npub use {}::*;\n",
            mod_name
        ));
    }
    // Only write when the module list changed — unconditional writes churn
    // mtime and make every `generate` look non-idempotent.
    let needs_write = if generated_mod.exists() {
        std::fs::read_to_string(&generated_mod)? != mod_content
    } else {
        true
    };
    if needs_write {
        std::fs::write(&generated_mod, mod_content)?;
    }

    Ok(())
}

/// Record source and generated bytes so reviewers can reproduce a generation
/// without treating target/ as authoritative source.
fn generation_manifest(root: &Path) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let generated = root.join(bloxide_codegen::blox_crate::GENERATED_WORKSPACE_DIR);
    if !generated.is_dir() {
        return Ok(());
    }
    let mut sources = BTreeMap::new();
    for (_, blox) in bloxide_codegen::util::discover_bloxes(root)? {
        sources.insert(
            blox.toml_path.display().to_string(),
            bloxide_codegen::packages::digest(&std::fs::read(&blox.toml_path)?),
        );
    }
    for entry in WalkDir::new(root)
        .max_depth(DISCOVERY_MAX_DEPTH)
        .into_iter()
        .filter_entry(|e| e.file_name() != "target")
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == "system.toml")
    {
        sources.insert(
            entry.path().display().to_string(),
            bloxide_codegen::packages::digest(&std::fs::read(entry.path())?),
        );
    }
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(&generated)
        .into_iter()
        .filter_entry(|e| e.file_name() != "target")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name() != "generation-manifest.json")
    {
        files.insert(
            entry.path().strip_prefix(&generated)?.display().to_string(),
            bloxide_codegen::packages::digest(&std::fs::read(entry.path())?),
        );
    }
    let bundle_lock = std::fs::read(root.join("blox.lock"))
        .ok()
        .map(|bytes| bloxide_codegen::packages::digest(&bytes));
    let manifest = serde_json::json!({"format":1,"hash_algorithm":"sha256","blox_lock_sha256":bundle_lock,"sources":sources,"generated":files});
    std::fs::write(
        generated.join("generation-manifest.json"),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    Ok(())
}

/// Every handwritten/generated workspace invoked by the full CLI workflow.
pub fn cargo_manifests(root: &Path) -> Vec<PathBuf> {
    let mut manifests = vec![root.join("Cargo.toml")];
    let generated = root
        .join(bloxide_codegen::blox_crate::GENERATED_WORKSPACE_DIR)
        .join("Cargo.toml");
    if generated.exists() {
        manifests.push(generated);
    }
    manifests.extend(
        bloxide_codegen::blox_crate::impl_crate_dirs(root)
            .into_iter()
            .map(|dir| root.join(dir).join("Cargo.toml"))
            .filter(|p| p.exists()),
    );
    manifests
}

pub fn verify_cargo_locks(root: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    let root = root.canonicalize()?;
    for manifest in cargo_manifests(&root) {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let location = std::process::Command::new(cargo)
            .current_dir(manifest.parent().unwrap())
            .args([
                "locate-project",
                "--workspace",
                "--message-format",
                "plain",
                "--manifest-path",
            ])
            .arg(&manifest)
            .output()?;
        anyhow::ensure!(
            location.status.success(),
            "cannot locate Cargo lock owner: {}",
            String::from_utf8_lossy(&location.stderr)
        );
        let owner = PathBuf::from(String::from_utf8(location.stdout)?.trim());
        let generated = owner.parent().unwrap()
            == root.join(bloxide_codegen::blox_crate::GENERATED_WORKSPACE_DIR);
        let lock = if generated {
            root.join("locks/blox-generated.Cargo.lock")
        } else {
            owner.with_file_name("Cargo.lock")
        };
        anyhow::ensure!(
            lock.is_file(),
            "missing {}; run cargo blox lock",
            lock.display()
        );
        let before = std::fs::read(&lock)?;
        let mut command = cargo_metadata::MetadataCommand::new();
        command
            .manifest_path(&manifest)
            .current_dir(manifest.parent().unwrap())
            .other_options(vec!["--locked".into(), "--offline".into()]);
        command.exec().with_context(|| {
            format!(
                "locked generation validation failed for {}",
                manifest.display()
            )
        })?;
        anyhow::ensure!(
            std::fs::read(&lock)? == before,
            "Cargo changed lock during verification: {}",
            lock.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod lock_tests {
    use super::*;
    #[test]
    fn member_impl_uses_cargo_workspace_lock_owner() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("member/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers=['member']\n[workspace.dependencies]\nbehavior={path='member'}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("member/Cargo.toml"),
            "[package]\nname='behavior'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(root.join("member/src/lib.rs"), "pub fn action() {}\n").unwrap();
        std::fs::write(root.join("system.toml"), "[system]\nname='fixture'\nruntime='tokio'\n[[actors]]\nname='actor'\nblox='actor-blox'\nimpl_crate='behavior'\n").unwrap();
        assert_eq!(cargo_manifests(root).len(), 2);
        assert!(std::process::Command::new("cargo")
            .current_dir(root)
            .args(["generate-lockfile", "--offline"])
            .status()
            .unwrap()
            .success());
        assert!(!root.join("member/Cargo.lock").exists());
        verify_cargo_locks(root).unwrap();
        assert!(!root.join("member/Cargo.lock").exists());
    }
}
