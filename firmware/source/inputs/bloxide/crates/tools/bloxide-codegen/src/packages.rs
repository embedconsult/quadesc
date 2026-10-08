// Copyright 2025 Bloxide, all rights reserved
//! Explicit, already-fetched declarative packages. No network resolver.
use crate::{
    schema::SystemConfig,
    util::{BloxSourceKind, DiscoveredBlox},
};
use anyhow::{bail, ensure, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Workspace {
    format: u32,
    packages: BTreeMap<String, Selection>,
    #[serde(default)]
    overrides: BTreeMap<String, Override>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    package: String,
    path: Option<String>,
    git: Option<String>,
    rev: Option<String>,
    checkout: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    package: Package,
    #[serde(default)]
    bloxes: Vec<BloxExport>,
    #[serde(default)]
    crates: Vec<CrateExport>,
    #[serde(default)]
    tools: Vec<CrateExport>,
    #[serde(default)]
    dependencies: BTreeMap<String, Dependency>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Package {
    name: String,
    version: String,
    blox_schema: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BloxExport {
    name: String,
    path: String,
    #[serde(rename = "crate")]
    crate_name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CrateExport {
    name: String,
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dependency {
    package: String,
    git: String,
    rev: String,
}
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct LockedPackage {
    alias: String,
    name: String,
    version: String,
    source: String,
    path: String,
    url: Option<String>,
    revision: Option<String>,
    tree: Option<String>,
    manifest_sha256: String,
    content_sha256: String,
    dirty: bool,
}
#[derive(Default)]
pub struct Composition {
    pub bloxes: BTreeMap<String, DiscoveredBlox>,
    pub source_roots: Vec<PathBuf>,
    pub blox_metadata: BTreeMap<String, (String, String, Option<String>)>,
    pub crates: BTreeMap<String, toml::Value>,
    pub qualified: BTreeMap<String, String>,
    pub patches: BTreeMap<String, BTreeMap<String, toml::Value>>,
    lock: String,
}
fn git(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    ensure!(
        output.status.success(),
        "git {} failed at {}: {}",
        args.join(" "),
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
// Reserved generator/Cargo outputs, independent of .gitignore.
fn output_path(root: &Path, relative: &Path) -> bool {
    if relative.starts_with(".git")
        || relative.starts_with("target")
        || relative == Path::new(".vscode/settings.json")
        || relative == Path::new("blox.lock")
        || relative == Path::new("locks/blox-generated.Cargo.lock")
    {
        return true;
    }
    for ancestor in relative.ancestors() {
        if ancestor.file_name().is_some_and(|n| n == "target")
            && root
                .join(ancestor.parent().unwrap())
                .join("Cargo.toml")
                .is_file()
        {
            return true;
        }
        if ancestor.ends_with("src/generated") {
            let dir = root.join(ancestor.parent().unwrap().parent().unwrap());
            if dir.join("Cargo.toml").is_file() && dir.join("blox.toml").is_file() {
                return true;
            }
        }
    }
    false
}
fn inside(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    let relative = Path::new(relative);
    ensure!(
        !relative.is_absolute()
            && relative.components().all(|c| matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )),
        "package path escapes root: {}",
        relative.display()
    );
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        ensure!(
            !path.is_symlink(),
            "symlink package input is unsupported: {}",
            path.display()
        );
    }
    let path = path
        .canonicalize()
        .with_context(|| format!("package input {}", relative.display()))?;
    ensure!(
        path.starts_with(root),
        "package path escapes root: {}",
        relative.display()
    );
    ensure!(
        !output_path(root, path.strip_prefix(root)?),
        "package export uses reserved output path: {}",
        relative.display()
    );
    Ok(path)
}

fn validate_cargo_inputs(root: &Path, manifest: &Path) -> anyhow::Result<()> {
    let cargo: toml::Value = toml::from_str(&std::fs::read_to_string(manifest)?)?;
    let mut paths = Vec::new();
    if let Some(build) = cargo
        .get("package")
        .and_then(|p| p.get("build"))
        .and_then(|p| p.as_str())
    {
        paths.push(build);
    }
    if let Some(lib) = cargo
        .get("lib")
        .and_then(|p| p.get("path"))
        .and_then(|p| p.as_str())
    {
        paths.push(lib);
    }
    for kind in ["bin", "example", "test", "bench"] {
        if let Some(targets) = cargo.get(kind).and_then(|v| v.as_array()) {
            paths.extend(
                targets
                    .iter()
                    .filter_map(|v| v.get("path").and_then(|v| v.as_str())),
            );
        }
    }
    for input in paths {
        let path = manifest.parent().unwrap().join(input);
        for ancestor in path.ancestors() {
            ensure!(
                !ancestor.is_symlink(),
                "symlink Cargo source input: {}",
                path.display()
            );
        }
        let canonical = path
            .canonicalize()
            .with_context(|| format!("Cargo source input {}", path.display()))?;
        ensure!(
            canonical.starts_with(root),
            "Cargo source input escapes package: {}",
            path.display()
        );
        ensure!(
            !output_path(root, canonical.strip_prefix(root)?),
            "Cargo source input uses reserved output path: {}",
            path.display()
        );
    }
    Ok(())
}

// Check literal Rust file references without pretending to execute macros or
// build scripts. Tokenization avoids treating comments/string contents as code.
fn validate_rust_inputs(root: &Path, file: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use proc_macro2::{Delimiter, TokenStream, TokenTree};
    fn literals(stream: TokenStream, out: &mut Vec<String>) {
        let tokens: Vec<_> = stream.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            if let TokenTree::Ident(name) = token {
                if matches!(
                    name.to_string().as_str(),
                    "include" | "include_str" | "include_bytes"
                ) && matches!(tokens.get(index + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!')
                {
                    if let Some(TokenTree::Group(group)) = tokens.get(index + 2) {
                        if let Ok(path) = syn::parse2::<syn::LitStr>(group.stream()) {
                            out.push(path.value());
                        }
                    }
                }
            }
            if let TokenTree::Group(group) = token {
                let attr: Vec<_> = group.stream().into_iter().collect();
                if group.delimiter() == Delimiter::Bracket
                    && attr.len() == 3
                    && matches!(&attr[0], TokenTree::Ident(i) if i == "path")
                    && matches!(&attr[1], TokenTree::Punct(p) if p.as_char() == '=')
                {
                    if let Ok(path) = syn::parse2::<syn::LitStr>(TokenStream::from(attr[2].clone()))
                    {
                        out.push(path.value());
                    }
                }
                literals(group.stream(), out);
            }
        }
    }
    let text = std::str::from_utf8(bytes)?;
    let tokens: TokenStream = text
        .parse()
        .map_err(|e| anyhow::anyhow!("cannot inventory Rust input {}: {e}", file.display()))?;
    let mut inputs = Vec::new();
    literals(tokens, &mut inputs);
    for input in inputs {
        let path = file.parent().unwrap().join(input);
        let mut normalized = PathBuf::new();
        for part in path.components() {
            match part {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                std::path::Component::CurDir => {}
                part => normalized.push(part.as_os_str()),
            }
        }
        ensure!(
            normalized.starts_with(root),
            "literal Rust input escapes package: {} in {}",
            normalized.display(),
            file.display()
        );
        ensure!(
            !output_path(root, normalized.strip_prefix(root)?),
            "literal Rust input uses reserved output path: {}",
            normalized.display()
        );
    }
    Ok(())
}

fn content_hash(root: &Path, pinned: bool) -> anyhow::Result<(String, bool)> {
    let mut hash = Sha256::new();
    let mut files = Vec::new();
    ensure!(
        !root.join(".gitmodules").exists(),
        "submodules require explicit source locking (unsupported)"
    );
    let tree = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-tree", "-rz", "HEAD"])
        .output()?;
    let mut tracked = BTreeMap::new();
    if tree.status.success() {
        for entry in tree.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()) {
            let entry = std::str::from_utf8(entry)?;
            let (header, name) = entry.split_once('\t').context("invalid Git tree entry")?;
            let fields: Vec<_> = header.split_whitespace().collect();
            ensure!(fields[0] != "160000", "submodules are unsupported: {name}");
            ensure!(
                fields[0] != "120000",
                "symlink package input is unsupported: {name}"
            );
            // A committed source cannot hide in an output directory.
            ensure!(
                !output_path(root, Path::new(name))
                    || matches!(name, "blox.lock" | "locks/blox-generated.Cargo.lock"),
                "tracked source uses reserved output path: {name}"
            );
            tracked.insert(
                PathBuf::from(name),
                (fields[0].to_string(), fields[2].to_string()),
            );
        }
    }
    if pinned {
        ensure!(
            git(root, &["rev-parse", "--show-toplevel"])? == root.display().to_string(),
            "Git package must select repository root"
        );
    }
    for entry in walkdir::WalkDir::new(root).into_iter().filter_entry(|e| {
        e.path() == root || !output_path(root, e.path().strip_prefix(root).unwrap())
    }) {
        let entry = entry?;
        if entry.path() == root {
            continue;
        }
        ensure!(
            entry.file_name() != ".git",
            "nested repository is unsupported: {}",
            entry.path().display()
        );
        ensure!(
            !entry.file_type().is_symlink(),
            "symlink package input is unsupported: {}",
            entry.path().display()
        );
        if !entry.file_type().is_dir() {
            ensure!(
                entry.file_type().is_file(),
                "non-file package input: {}",
                entry.path().display()
            );
            files.push(entry.into_path());
        }
    }
    if pinned {
        for path in tracked.keys().filter(|path| !output_path(root, path)) {
            ensure!(
                root.join(path).is_file(),
                "missing pinned source input: {}",
                path.display()
            );
        }
    }
    files.sort();
    let mut untracked = false;
    for chunk in files.chunks(100) {
        // hash-object reads actual disk bytes (not the index/stat cache).
        let blobs = if pinned {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["hash-object", "--no-filters", "--"])
                .args(chunk)
                .output()?;
            ensure!(output.status.success(), "cannot hash pinned package files");
            String::from_utf8(output.stdout)?
        } else {
            String::new()
        };
        let mut blobs = blobs.lines();
        for path in chunk {
            let relative = path.strip_prefix(root)?;
            untracked |= !tracked.contains_key(relative);
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(path)?.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            if pinned {
                let (mode, blob) = tracked.get(relative).with_context(|| {
                    format!(
                        "Git source input is not in pinned tree: {}",
                        relative.display()
                    )
                })?;
                ensure!(
                    Some(blob.as_str()) == blobs.next() && (mode == "100755") == executable,
                    "Git source input differs from pinned tree: {}",
                    relative.display()
                );
            }
            let bytes = std::fs::read(path)?;
            if path.extension().is_some_and(|ext| ext == "rs") {
                validate_rust_inputs(root, path, &bytes)?;
            }
            hash.update(relative.as_os_str().as_encoded_bytes());
            hash.update([0, u8::from(executable)]);
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
    }
    Ok((format!("{:x}", hash.finalize()), untracked))
}

pub fn load(root: &Path) -> anyhow::Result<Composition> {
    load_inner(root, false)
}
fn load_inner(root: &Path, identity: bool) -> anyhow::Result<Composition> {
    let path = root.join("blox-workspace.toml");
    if !path.exists() {
        return Ok(Composition::default());
    }
    let workspace: Workspace = toml::from_str(&std::fs::read_to_string(path)?)?;
    ensure!(workspace.format == 1, "unsupported blox-workspace format");
    for alias in workspace.overrides.keys() {
        ensure!(
            workspace.packages.contains_key(alias),
            "override of unknown package '{alias}'"
        );
    }
    let mut result = Composition::default();
    let mut locked = Vec::new();
    let mut requirements = BTreeMap::new();
    let mut identities = BTreeMap::new();
    let mut exported_names = BTreeSet::new();
    let mut owners = BTreeMap::new();
    for (alias, selection) in &workspace.packages {
        let override_path = workspace.overrides.get(alias).map(|o| o.path.as_str());
        let local = override_path.or(selection.path.as_deref());
        ensure!(
            selection.path.is_none()
                || (selection.git.is_none()
                    && selection.rev.is_none()
                    && selection.checkout.is_none()),
            "path selection must not claim Git provenance"
        );
        let selected = local.or(selection.checkout.as_deref()).ok_or_else(|| {
            anyhow::anyhow!("package '{alias}' needs an explicit already-fetched path/checkout")
        })?;
        let package_root = root.join(selected).canonicalize()?;
        result.source_roots.push(package_root.clone());
        let text = std::fs::read_to_string(package_root.join("blox-package.toml"))?;
        let manifest: Manifest =
            toml::from_str(&text).with_context(|| format!("package '{alias}'"))?;
        ensure!(
            manifest.format == 1 && manifest.package.blox_schema == 1,
            "unsupported package/schema format"
        );
        ensure!(
            manifest.package.name == selection.package,
            "package identity mismatch for '{alias}'"
        );
        ensure!(
            identities
                .insert(selection.package.clone(), alias.clone())
                .is_none(),
            "duplicate package identity '{}'",
            selection.package
        );
        let mut revision: Option<String> = None;
        let mut tree: Option<String> = None;
        let mut dirty = git(
            &package_root,
            &["status", "--porcelain", "--untracked-files=all", "--", "."],
        )
        .map(|s| !s.is_empty())
        .unwrap_or(true);
        if local.is_none() {
            let rev = selection
                .rev
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Git package needs full rev"))?;
            ensure!(
                rev.len() == 40 && rev.bytes().all(|c| c.is_ascii_hexdigit()),
                "Git revision must be a full immutable commit"
            );
            let url = selection
                .git
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Git package needs canonical URL"))?;
            ensure!(
                git(&package_root, &["remote", "get-url", "origin"])? == url,
                "Git URL mismatch for '{alias}'"
            );
            ensure!(
                git(&package_root, &["rev-parse", "HEAD"])? == rev,
                "Git revision mismatch for '{alias}'"
            );
            ensure!(
                !dirty,
                "Git package '{alias}' is dirty; declare a path override"
            );
            ensure!(
                !package_root.join(".gitmodules").exists(),
                "submodules require explicit source locking (unsupported)"
            );
            revision = Some(rev.into());
            tree = Some(git(&package_root, &["rev-parse", "HEAD^{tree}"])?);
        }
        for export in manifest.crates.iter().chain(&manifest.tools) {
            ensure!(
                exported_names.insert(export.name.clone()),
                "duplicate exported Cargo name '{}'",
                export.name
            );
            let cargo = inside(&package_root, &export.path)?;
            validate_cargo_inputs(&package_root, &cargo)?;
            ensure!(
                crate::util::parse_package_name(&cargo).as_deref() == Some(&export.name),
                "exported Cargo name mismatch: {}",
                export.name
            );
            let mut dep = toml::Table::new();
            if local.is_some() {
                dep.insert(
                    "path".into(),
                    cargo.parent().unwrap().display().to_string().into(),
                );
            } else {
                dep.insert("git".into(), selection.git.clone().unwrap().into());
                dep.insert("rev".into(), revision.clone().unwrap().into());
            }
            let dep = toml::Value::Table(dep);
            if override_path.is_some() {
                if let Some(url) = &selection.git {
                    result
                        .patches
                        .entry(url.clone())
                        .or_default()
                        .insert(export.name.clone(), dep.clone());
                }
            }
            owners.insert(export.name.clone(), alias.clone());
            result.crates.insert(export.name.clone(), dep);
            let blox_path = cargo.parent().unwrap().join("blox.toml");
            if blox_path.exists() {
                result.bloxes.insert(
                    export.name.clone(),
                    DiscoveredBlox {
                        config: toml::from_str(&std::fs::read_to_string(&blox_path)?)?,
                        toml_path: blox_path,
                        kind: BloxSourceKind::InCrate,
                    },
                );
            }
        }
        let mut symbols = BTreeSet::new();
        for export in &manifest.bloxes {
            ensure!(
                symbols.insert(&export.name),
                "duplicate blox symbol '{}'",
                export.name
            );
            ensure!(
                exported_names.insert(export.crate_name.clone()),
                "duplicate exported Cargo name '{}'",
                export.crate_name
            );
            let toml_path = inside(&package_root, &export.path)?;
            ensure!(
                !toml_path.parent().unwrap().join("Cargo.toml").exists(),
                "pure TOML export must not contain Cargo.toml"
            );
            let config = toml::from_str(&std::fs::read_to_string(&toml_path)?)?;
            ensure!(
                crate::util::pure_toml_crate_name(&config).as_deref() == Some(&export.crate_name),
                "blox export crate name must match actor name"
            );
            result.qualified.insert(
                format!("{alias}::{}", export.name),
                export.crate_name.clone(),
            );
            owners.insert(export.crate_name.clone(), alias.clone());
            let (_, edition, license) = crate::blox_crate::workspace_package_values(&package_root);
            result.blox_metadata.insert(
                export.crate_name.clone(),
                (manifest.package.version.clone(), edition, license),
            );
            result.bloxes.insert(
                export.crate_name.clone(),
                DiscoveredBlox {
                    config,
                    toml_path,
                    kind: BloxSourceKind::PureToml,
                },
            );
        }
        let content = if identity {
            let (content, untracked) = content_hash(&package_root, local.is_none())?;
            dirty |= untracked;
            content
        } else {
            String::new()
        };
        requirements.insert(alias.clone(), manifest.dependencies);
        locked.push(LockedPackage {
            alias: alias.clone(),
            name: manifest.package.name,
            version: manifest.package.version,
            source: if local.is_some() { "path" } else { "git" }.into(),
            path: selected.into(),
            url: if local.is_some() {
                None
            } else {
                selection.git.clone()
            },
            revision,
            tree,
            manifest_sha256: digest(text.as_bytes()),
            content_sha256: content,
            dirty,
        });
    }
    let workspace_cargo: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml"))?)?;
    for (name, blox) in &result.bloxes {
        let Some(owner) = owners.get(name) else {
            continue;
        };
        let mut references = crate::blox_crate::referenced_crates(&blox.config)
            .into_iter()
            .map(|s| s.replace('_', "-"))
            .collect::<BTreeSet<_>>();
        if let Some(package) = &blox.config.package {
            references.extend(package.dependencies.keys().cloned());
            references.extend(package.dev_dependencies.keys().cloned());
        }
        for reference in references {
            let reference = workspace_cargo
                .get("workspace")
                .and_then(|w| w.get("dependencies"))
                .and_then(|d| d.get(&reference))
                .and_then(|d| d.get("package"))
                .and_then(|p| p.as_str())
                .unwrap_or(&reference);
            // The generated engine dependency is intrinsic, not a domain
            // reference in the declarative source.
            if reference == "bloxide-core" {
                continue;
            }
            if let Some(other) = owners.get(&reference.replace('_', "-")) {
                if other != owner {
                    let identity = &workspace.packages[other].package;
                    ensure!(requirements[owner].values().any(|d| &d.package == identity), "package '{owner}' references '{reference}' without a declared bundle dependency");
                }
            }
        }
    }
    let mut edges = BTreeMap::<String, Vec<String>>::new();
    for (alias, deps) in requirements {
        for dependency in deps.values() {
            let selected_alias = identities.get(&dependency.package).ok_or_else(|| {
                anyhow::anyhow!(
                    "package '{alias}' requires explicitly selected '{}'",
                    dependency.package
                )
            })?;
            let selection = &workspace.packages[selected_alias];
            ensure!(
                selection.git.as_deref() == Some(&dependency.git)
                    && selection.rev.as_deref() == Some(&dependency.rev),
                "conflicting source revision for '{}'",
                dependency.package
            );
            edges
                .entry(alias.clone())
                .or_default()
                .push(selected_alias.clone());
        }
    }
    fn visit(
        node: &str,
        edges: &BTreeMap<String, Vec<String>>,
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> anyhow::Result<()> {
        if done.contains(node) {
            return Ok(());
        }
        ensure!(
            stack.insert(node.into()),
            "package dependency cycle at '{node}'"
        );
        if let Some(children) = edges.get(node) {
            for child in children {
                visit(child, edges, stack, done)?;
            }
        }
        stack.remove(node);
        done.insert(node.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for node in workspace.packages.keys() {
        visit(node, &edges, &mut BTreeSet::new(), &mut done)?;
    }
    if !identity {
        return Ok(result);
    }
    #[derive(Serialize)]
    #[serde(rename_all = "kebab-case")]
    struct Lock {
        format: u32,
        blox_schema: u32,
        generator_revision: String,
        generator_sha256: String,
        packages: Vec<LockedPackage>,
    }
    result.lock = toml::to_string(&Lock {
        format: 1,
        blox_schema: 1,
        generator_revision: env!("BLOXIDE_GENERATOR_REVISION").into(),
        generator_sha256: env!("BLOXIDE_GENERATOR_SHA256").into(),
        packages: locked,
    })?;
    Ok(result)
}

pub fn resolve(root: &Path) -> anyhow::Result<()> {
    let composition = load_inner(root, true)?;
    ensure!(
        !composition.lock.is_empty(),
        "resolve requires blox-workspace.toml"
    );
    std::fs::write(root.join("blox.lock"), composition.lock)?;
    Ok(())
}
pub fn verify(root: &Path) -> anyhow::Result<()> {
    let composition = load_inner(root, true)?;
    if composition.lock.is_empty() {
        return Ok(());
    }
    let actual = std::fs::read_to_string(root.join("blox.lock"))
        .context("missing blox.lock; run cargo blox resolve")?;
    ensure!(actual == composition.lock, "blox.lock differs from source/generator identity; review changes and run cargo blox resolve");
    Ok(())
}
pub fn normalize(config: &mut SystemConfig, root: &Path) -> anyhow::Result<()> {
    let composition = load(root)?;
    for actor in &mut config.actors {
        if actor.blox.contains("::") {
            actor.blox = composition
                .qualified
                .get(&actor.blox)
                .ok_or_else(|| anyhow::anyhow!("unknown qualified blox '{}'", actor.blox))?
                .clone();
        } else if composition
            .qualified
            .values()
            .any(|name| name == &actor.blox)
        {
            bail!(
                "imported pure-TOML blox '{}' requires a qualified package::export reference",
                actor.blox
            );
        }
    }
    Ok(())
}

/// Cargo remains the Rust resolver; preserve its complete source tables.
pub fn dependencies(root: &Path) -> anyhow::Result<BTreeMap<String, toml::Value>> {
    let cargo: toml::Value = toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml"))?)?;
    let mut result: BTreeMap<String, toml::Value> = cargo
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|v| v.as_table())
        .map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    for (name, value) in load(root)?.crates {
        if let Some(existing) = result.get(&name) {
            let existing_path = existing.get("path").and_then(|v| v.as_str());
            let selected_path = value.get("path").and_then(|v| v.as_str());
            if let (Some(a), Some(b)) = (existing_path, selected_path) {
                ensure!(
                    root.join(a).canonicalize()? == Path::new(b).canonicalize()?,
                    "conflicting Cargo source for '{name}'"
                );
            } else {
                ensure!(
                    existing.get("git") == value.get("git")
                        && existing.get("rev") == value.get("rev"),
                    "conflicting Cargo source for '{name}'"
                );
            }
        } else {
            result.insert(name, value);
        }
    }
    Ok(result)
}
pub fn dependency_line(
    root: &Path,
    prefix: &str,
    name: &str,
    features: &[String],
    no_default: bool,
) -> anyhow::Result<String> {
    let deps = dependencies(root)?;
    let value = deps.get(name).ok_or_else(|| anyhow::anyhow!("dependency \"{name}\" has no entry in the workspace root's [workspace.dependencies] or package exports"))?;
    let mut table = match value {
        toml::Value::String(version) => {
            let mut t = toml::Table::new();
            t.insert("version".into(), version.clone().into());
            t
        }
        toml::Value::Table(t) => t.clone(),
        _ => bail!("invalid Cargo dependency '{name}'"),
    };
    if let Some(path) = table.get("path").and_then(|p| p.as_str()) {
        let path = if Path::new(path).is_absolute() {
            path.to_string()
        } else {
            format!("{prefix}{path}")
        };
        table.insert("path".into(), path.into());
    }
    let mut selected = table
        .get("features")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for feature in features {
        let feature = toml::Value::String(feature.clone());
        if !selected.contains(&feature) {
            selected.push(feature);
        }
    }
    if !selected.is_empty() {
        table.insert("features".into(), selected.into());
    }
    if no_default {
        table.insert("default-features".into(), false.into());
    }
    let mut fields = Vec::new();
    // Keep dependency sources first for readable generated manifests.
    for key in [
        "path",
        "git",
        "rev",
        "version",
        "package",
        "default-features",
        "features",
    ] {
        if let Some(value) = table.remove(key) {
            fields.push(format!("{key} = {value}"));
        }
    }
    fields.extend(table.iter().map(|(key, value)| format!("{key} = {value}")));
    let fields = fields.join(", ");
    Ok(format!("{name} = {{ {fields} }}\n"))
}
