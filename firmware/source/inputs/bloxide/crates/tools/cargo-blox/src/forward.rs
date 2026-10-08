// Copyright 2025 Bloxide, all rights reserved
//! Forward commands to `cargo`.

use clap_cargo::Features;
use std::path::Path;
use std::process::Command;

pub fn forward_to_cargo(cmd: &str, extra_args: &[String]) -> anyhow::Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let mut command = Command::new(&cargo);
    command.arg(cmd);

    for arg in extra_args {
        command.arg(arg);
    }

    let status = command.status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Generate code from blox.toml, then forward to a cargo subcommand.
///
/// Shared implementation for `build`, `check`, `test`, and `run` — they all
/// regenerate first, then forward to cargo with the same feature-flag
/// handling.
///
/// Without `--example`, `build` / `check` / `test` run in BOTH workspaces:
/// the repo workspace (`cargo <cmd> --workspace`, from the current dir —
/// feature flags and extra args apply here only) and the generated workspace
/// (`cargo <cmd> --workspace --manifest-path <root>/target/bloxide-generated/Cargo.toml`,
/// no extra args). `test` additionally runs each impl crate's tests via
/// `--manifest-path` — impl crates belong to neither workspace (see
/// `bloxide_codegen::blox_crate::impl_crate_dirs`).
///
/// With `--example <name>`, the command is scoped to that example crate in
/// the generated workspace (`cargo <cmd> --manifest-path <generated> -p <name>`).
/// `run` REQUIRES `--example` — examples are no longer root workspace
/// members; trailing args are passed to the binary after `--`.
pub fn generate_then_forward(
    cmd: &str,
    cargo: Features,
    example: Option<String>,
    args: Vec<String>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        cmd == "run"
            || !args
                .iter()
                .any(|a| a == "--manifest-path" || a.starts_with("--manifest-path=")),
        "cargo-blox owns manifest selection"
    );
    crate::generate::generate(None)?;

    let root = crate::utils::find_workspace_root()?;
    if cmd != "run"
        && args
            .iter()
            .take_while(|a| a.as_str() != "--")
            .any(|a| matches!(a.as_str(), "--locked" | "--frozen"))
    {
        crate::generate::verify_cargo_locks(&root)?;
    }
    let generated_manifest = root
        .join(bloxide_codegen::blox_crate::GENERATED_WORKSPACE_DIR)
        .join("Cargo.toml");

    if let Some(name) = example {
        return forward_to_example(cmd, cargo, &name, args, &root, &generated_manifest);
    }

    if cmd == "run" {
        anyhow::bail!(
            "`cargo blox run` requires `--example <name>`: examples are no longer root\n\
             workspace members — they are materialized into \
             target/bloxide-generated/examples/.\n\
             Available examples: {}",
            crate::utils::available_examples(&root).join(", ")
        );
    }

    // Build the complete invocation list, then preflight all graphs before
    // compiling any of them. Each preflight receives exactly that invocation's
    // graph-affecting flags.
    let mut common = Vec::new();
    push_feature_flags(&cargo, &mut common);
    common.extend(args);
    let mut root_args = vec!["--workspace".into()];
    root_args.extend(common.clone());
    let mut invocations = vec![(root.join("Cargo.toml"), root_args)];
    if generated_manifest.exists() {
        let mut generated_args = vec![
            "--workspace".into(),
            "--manifest-path".into(),
            generated_manifest.display().to_string(),
        ];
        generated_args.extend(common.clone());
        invocations.push((generated_manifest, generated_args));
    }
    if cmd == "test" {
        for dir in bloxide_codegen::blox_crate::impl_crate_dirs(&root) {
            let manifest = root.join(dir).join("Cargo.toml");
            if manifest.exists() {
                let mut impl_args = vec!["--manifest-path".into(), manifest.display().to_string()];
                impl_args.extend(common.clone());
                invocations.push((manifest, impl_args));
            }
        }
    }
    for (manifest, args) in &invocations {
        verify_composed_cargo_graph(&root, manifest, args, cmd)?;
    }
    let previous_dir = std::env::current_dir()?;
    for (manifest, args) in invocations {
        std::env::set_current_dir(manifest.parent().unwrap())?;
        let result = forward_to_cargo(cmd, &args);
        std::env::set_current_dir(&previous_dir)?;
        result?;
    }
    Ok(())
}

/// Forward a command scoped to one example crate in the generated workspace:
/// `cargo <cmd> --manifest-path <generated> -p <name> [--features ...] [-- <args>]`.
fn forward_to_example(
    cmd: &str,
    cargo: Features,
    name: &str,
    args: Vec<String>,
    root: &Path,
    generated_manifest: &Path,
) -> anyhow::Result<()> {
    let example_dir = generated_manifest
        .parent()
        .unwrap()
        .join("examples")
        .join(name);
    if !example_dir.is_dir() {
        anyhow::bail!(crate::exit::not_found(format!(
            "example '{}' not found (expected {}).\nAvailable examples: {}",
            name,
            example_dir.display(),
            crate::utils::available_examples(root).join(", ")
        )));
    }

    let mut extra = vec![
        "--manifest-path".to_string(),
        generated_manifest.display().to_string(),
        "-p".to_string(),
        name.to_string(),
    ];
    push_feature_flags(&cargo, &mut extra);
    if cmd == "run" && !args.is_empty() {
        extra.push("--".to_string());
    }
    let source_target = walkdir::WalkDir::new(root)
        .max_depth(crate::utils::DISCOVERY_MAX_DEPTH)
        .into_iter()
        .filter_entry(|e| e.file_name() != "target")
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == "system.toml")
        .find_map(|entry| {
            let text = std::fs::read_to_string(entry.path()).ok()?;
            let config: bloxide_codegen::schema::SystemConfig = toml::from_str(&text).ok()?;
            (bloxide_codegen::example_crate::example_crate_name(&config, entry.path()) == name)
                .then_some(config.system.target)
                .flatten()
        });
    if let Some(target) = source_target {
        anyhow::ensure!(
            cmd != "run",
            "embedded binaries require a platform runner; cargo blox run is hosted-only"
        );
        anyhow::ensure!(
            !args
                .iter()
                .any(|a| a == "--target" || a.starts_with("--target=")),
            "embedded target comes from system.toml"
        );
        extra.extend(["--target".into(), target]);
    }
    extra.extend(args);
    verify_composed_cargo_graph(root, generated_manifest, &extra, cmd)?;
    // Cargo reads configuration from cwd, not --manifest-path. The generated
    // workspace owns entrypoint link flags; target selection remains explicit.
    std::env::set_current_dir(generated_manifest.parent().unwrap())?;
    println!("bloxide: cargo {} -p {} (generated examples)", cmd, name);
    forward_to_cargo(cmd, &extra)
}

/// clap-cargo feature flags → cargo CLI args.
fn push_feature_flags(cargo: &Features, extra: &mut Vec<String>) {
    if !cargo.features.is_empty() {
        extra.push("--features".to_string());
        extra.push(cargo.features.join(","));
    }
    if cargo.no_default_features {
        extra.push("--no-default-features".to_string());
    }
    if cargo.all_features {
        extra.push("--all-features".to_string());
    }
}

/// Refuse duplicate public contract crates or a Cargo source different from
/// the selected bundle. Cargo itself resolves the graph; this only checks it.
fn verify_composed_cargo_graph(
    root: &Path,
    manifest: &Path,
    args: &[String],
    cmd: &str,
) -> anyhow::Result<()> {
    if !root.join("blox-workspace.toml").exists() {
        return Ok(());
    }
    let selected = bloxide_codegen::packages::load(root)?;
    let (tree_flags, metadata_flags) = graph_flags(args)?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let cwd = manifest.parent().unwrap();
    let edges = if cmd == "test"
        || args.iter().take_while(|a| a.as_str() != "--").any(|a| {
            matches!(
                a.split('=').next().unwrap(),
                "--all-targets"
                    | "--tests"
                    | "--examples"
                    | "--benches"
                    | "--test"
                    | "--example"
                    | "--bench"
            )
        }) {
        "normal,build,dev"
    } else {
        "normal,build"
    };
    let tree = Command::new(cargo)
        .current_dir(cwd)
        .args(["tree", "--manifest-path"])
        .arg(manifest)
        .args([
            "--prefix", "none", "--format", "{p}", "--color", "never", "--edges", edges,
        ])
        .args(tree_flags)
        .output()?;
    anyhow::ensure!(
        tree.status.success(),
        "Cargo graph selection failed: {}",
        String::from_utf8_lossy(&tree.stderr)
    );
    let tree = String::from_utf8(tree.stdout)?;
    let active: std::collections::BTreeSet<_> = tree
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_end_matches(" (*)").replace(" (proc-macro)", ""))
        .collect();
    let mut metadata = cargo_metadata::MetadataCommand::new();
    metadata
        .manifest_path(manifest)
        .current_dir(cwd)
        .other_options(metadata_flags);
    let graph = metadata.exec()?;
    // tree applies Cargo's package, feature, host/build and target conditions.
    // metadata supplies full immutable IDs for only the packages tree selected.
    // Do not filter metadata by the target: its package inventory must also
    // contain build dependencies compiled for the host during cross builds.
    let packages: Vec<_> = graph
        .packages
        .iter()
        .filter(|p| active.contains(&tree_identity(p)))
        .collect();
    for line in &active {
        anyhow::ensure!(
            packages.iter().any(|p| tree_identity(p) == *line),
            "cannot identify selected Cargo package: {line}"
        );
    }
    let app_root = root.canonicalize()?;
    for package in &packages {
        if package.source.is_none() {
            let path = package.manifest_path.as_std_path().canonicalize()?;
            anyhow::ensure!(
                path.starts_with(&app_root)
                    || selected
                        .source_roots
                        .iter()
                        .any(|root| path.starts_with(root)),
                "unlocked external Cargo path source '{}'",
                package.name
            );
        }
    }
    for (name, source) in &selected.crates {
        let entries = packages
            .iter()
            .filter(|p| &p.name == name)
            .collect::<Vec<_>>();
        anyhow::ensure!(
            entries.len() <= 1,
            "Cargo graph contains multiple sources for public contract crate '{name}'"
        );
        let Some(package) = entries.first() else {
            continue;
        };
        if let Some(path) = source.get("path").and_then(|p| p.as_str()) {
            anyhow::ensure!(
                package.manifest_path.as_std_path().canonicalize()?
                    == Path::new(path).join("Cargo.toml").canonicalize()?,
                "Cargo graph source mismatch for '{name}'"
            );
        } else if let Some(url) = source.get("git").and_then(|p| p.as_str()) {
            let rev = source["rev"].as_str().unwrap();
            let identity = format!("git+{url}?rev={rev}#{rev}");
            anyhow::ensure!(
                package.source.as_ref().map(|s| s.repr.as_str()) == Some(identity.as_str()),
                "Cargo graph Git identity mismatch for '{name}'"
            );
        }
    }
    Ok(())
}

// Cargo's documented {p} tree field: name/version, then path or Git source.
fn tree_identity(package: &cargo_metadata::Package) -> String {
    let mut id = format!("{} v{}", package.name, package.version);
    if let Some(source) = &package.source {
        if let Some(git) = source.repr.strip_prefix("git+") {
            let (url, rev) = git.rsplit_once('#').unwrap_or((git, ""));
            id.push_str(&format!(" ({url}#{})", &rev[..rev.len().min(8)]));
        }
    } else {
        id.push_str(&format!(" ({})", package.manifest_path.parent().unwrap()));
    }
    id
}

fn graph_flags(args: &[String]) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let mut tree = Vec::new();
    let mut metadata = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        let both = matches!(
            key,
            "--features"
                | "-F"
                | "--all-features"
                | "--no-default-features"
                | "--locked"
                | "--offline"
                | "--frozen"
                | "--config"
        );
        let selection = matches!(key, "--package" | "-p" | "--workspace" | "--exclude");
        if both || selection || key == "--target" {
            let value = if matches!(
                key,
                "--features" | "-F" | "--config" | "--package" | "-p" | "--exclude" | "--target"
            ) {
                Some(
                    inline
                        .map(String::from)
                        .or_else(|| iter.next().cloned())
                        .ok_or_else(|| anyhow::anyhow!("missing value for {key}"))?,
                )
            } else {
                None
            };
            tree.push(key.into());
            if let Some(value) = &value {
                tree.push(value.clone());
            }
            if both {
                metadata.push(key.into());
                if let Some(value) = value {
                    metadata.push(value);
                }
            }
        } else if key == "--manifest-path" {
            anyhow::ensure!(
                inline.is_none(),
                "manifest selection is owned by cargo-blox"
            );
            iter.next(); // The invocation's generated/impl manifest.
        } else if arg.starts_with("-p") || arg.starts_with("-F") {
            anyhow::bail!("use separate values for -p and -F during graph validation");
        }
    }
    Ok((tree, metadata))
}
