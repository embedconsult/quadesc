// Copyright 2025 Bloxide, all rights reserved
//! Validation of a `system.toml` wiring manifest against the discovered blox
//! configs, run before any main.rs emission.

use super::actor_kind::skip_in_main_body;
use super::ctor_fields::{collect_all_ctor_field_names, collect_ctor_fields};
use crate::schema::{BloxConfig, SystemConfig};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn validate(
    config: &SystemConfig,
    blox_configs: &BTreeMap<String, BloxConfig>,
    active_features: &BTreeMap<String, BTreeSet<String>>,
) -> anyhow::Result<()> {
    validate_platform(config)?;
    let actor_names: BTreeSet<String> = config.actors.iter().map(|a| a.name.clone()).collect();

    // The supervisor is an implicit actor — its control_ref and notify_ref are
    // available for injection via `source = "actor", actor = "supervisor"`.
    let has_supervisor = !config.supervision.is_empty();

    for actor in &config.actors {
        if skip_in_main_body(actor) {
            continue;
        }
        if !blox_configs.contains_key(&actor.blox) {
            anyhow::bail!(
                "actor '{}' references unknown blox '{}'",
                actor.name,
                actor.blox
            );
        }
    }

    for actor in &config.actors {
        for (field_name, source) in &actor.inject {
            if source.source == "actor" {
                let src = source.actor.as_deref().unwrap_or("");
                if src == "supervisor" && has_supervisor {
                    // OK — implicit supervisor actor.
                    continue;
                }
                if !actor_names.contains(src) {
                    anyhow::bail!(
                        "actor '{}' inject field '{}' references unknown actor '{}'",
                        actor.name,
                        field_name,
                        src
                    );
                }
            }
        }
    }

    for actor in &config.actors {
        if skip_in_main_body(actor) {
            continue;
        }
        let blox_config = blox_configs.get(&actor.blox).ok_or_else(|| {
            anyhow::anyhow!(
                "actor '{}' references unknown blox '{}'",
                actor.name,
                actor.blox
            )
        })?;
        let ctor_fields = collect_ctor_fields(blox_config, &actor.blox, active_features);
        let ctor_names: BTreeSet<String> = ctor_fields.iter().map(|f| f.name.clone()).collect();
        let all_ctor_names = collect_all_ctor_field_names(blox_config);

        for field_name in actor.inject.keys() {
            if !ctor_names.contains(field_name) {
                anyhow::ensure!(
                    actor.inject[field_name].source != "resource",
                    "owned resource injection requires an enabled constructor field '{field_name}'"
                );
                // Tolerated only when the field exists but its feature gate is
                // off — the injection is cfg'd out together with the field.
                // Anything else (typically a typo) is a hard error.
                if !all_ctor_names.contains(field_name) {
                    anyhow::bail!(
                        "actor '{}' injects unknown constructor field '{}' (blox '{}')",
                        actor.name,
                        field_name,
                        actor.blox
                    );
                }
            }
        }

        for field in &ctor_fields {
            if field.is_self_id {
                continue;
            }
            if !actor.inject.contains_key(&field.name) {
                anyhow::bail!(
                    "actor '{}' constructor field '{}' has no inject entry",
                    actor.name,
                    field.name
                );
            }
        }
    }

    Ok(())
}

fn validate_platform(config: &SystemConfig) -> anyhow::Result<()> {
    use anyhow::ensure;
    ensure!(
        matches!(config.system.profile.as_str(), "hosted" | "embedded"),
        "unknown system profile"
    );
    let embedded = config.system.profile == "embedded";
    if embedded {
        ensure!(
            config.system.runtime == "embassy",
            "embedded profile requires embassy runtime"
        );
        ensure!(
            config.system.target.as_deref() == Some("thumbv8m.main-none-eabi"),
            "embedded target must be thumbv8m.main-none-eabi"
        );
        let platform = config
            .platform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("embedded profile requires platform"))?;
        ensure!(
            platform.entry == "cortex-m-rt",
            "embedded platform entry must be cortex-m-rt"
        );
        ensure!(
            platform.cleanup == "process-lifetime",
            "embedded cleanup must be process-lifetime"
        );
        for feature in config
            .system
            .executor_features
            .iter()
            .chain(&config.system.time_features)
        {
            ensure!(
                !feature.contains("std")
                    && (!feature.starts_with("arch-") || feature == "arch-cortex-m"),
                "host feature '{feature}' forbidden in embedded profile"
            );
        }
        ensure!(
            !config
                .actors
                .iter()
                .any(|a| matches!(a.kind.as_deref(), Some("dynamic" | "timer"))),
            "embedded dynamic actors and generic timer service require a bounded allocation audit"
        );
    } else {
        ensure!(
            config.system.target.is_none(),
            "target is only valid for embedded profile"
        );
    }
    if let Some(p) = &config.platform {
        ensure!(
            config.system.runtime == "embassy",
            "platform services currently require embassy runtime"
        );
        for value in [&p.impl_crate, &p.init, &p.on_error, &p.freeze, &p.resources] {
            syn::parse_str::<syn::Ident>(value)
                .map_err(|_| anyhow::anyhow!("invalid platform identifier '{value}'"))?;
        }
    }
    ensure!(
        config.services.is_empty() || config.platform.is_some(),
        "services require a platform"
    );
    let mut names = BTreeSet::new();
    let mut resources = BTreeSet::new();
    for actor in &config.actors {
        ensure!(
            names.insert(actor.name.as_str()),
            "duplicate instance name '{}'",
            actor.name
        );
    }
    for service in &config.services {
        ensure!(
            names.insert(service.name.as_str()),
            "duplicate instance name '{}'",
            service.name
        );
        for value in [
            &service.name,
            &service.impl_crate,
            &service.start,
            &service.resource,
        ] {
            syn::parse_str::<syn::Ident>(value)
                .map_err(|_| anyhow::anyhow!("invalid service identifier '{value}'"))?;
        }
        ensure!(
            service.message_path.is_some() == service.capacity.is_some(),
            "service '{}' message_path and capacity must both be present or absent",
            service.name
        );
        ensure!(
            service.capacity != Some(0),
            "service capacity must be positive"
        );
        if let Some(path) = &service.message_path {
            syn::parse_str::<syn::Type>(path)?;
        }
        ensure!(
            resources.insert(service.resource.as_str()),
            "resource '{}' consumed more than once",
            service.resource
        );
    }
    for source in config
        .actors
        .iter()
        .flat_map(|a| a.inject.values())
        .chain(config.services.iter().flat_map(|s| s.inject.values()))
    {
        match source.source.as_str() {
            "resource" => {
                ensure!(
                    config.platform.is_some(),
                    "resource injection requires platform"
                );
                let name = source
                    .field
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("resource injection requires field"))?;
                syn::parse_str::<syn::Ident>(name)?;
                ensure!(
                    resources.insert(name),
                    "resource '{name}' consumed more than once"
                );
            }
            "service" => {
                let name = source
                    .service
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("service injection requires service"))?;
                let service = config
                    .services
                    .iter()
                    .find(|s| s.name == name)
                    .ok_or_else(|| anyhow::anyhow!("unknown service '{name}'"))?;
                ensure!(
                    service.message_path.is_some(),
                    "cannot inject ActorRef for mailbox-free service '{name}'"
                );
            }
            _ => {}
        }
    }
    Ok(())
}
