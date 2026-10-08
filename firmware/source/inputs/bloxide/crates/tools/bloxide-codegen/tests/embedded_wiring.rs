// Copyright 2025 Bloxide, all rights reserved
use bloxide_codegen::{
    schema::{BloxConfig, SystemConfig},
    system_wiring,
};
use std::collections::BTreeMap;

fn system(extra: &str) -> SystemConfig {
    toml::from_str(&format!(
        r#"
[system]
name = "embedded-test"
runtime = "embassy"
profile = "embedded"
target = "thumbv8m.main-none-eabi"
[platform]
impl_crate = "fixture_platform"
entry = "cortex-m-rt"
init = "initialize"
on_error = "startup_failed"
freeze = "freeze"
resources = "Resources"
cleanup = "process-lifetime"
{extra}
"#
    ))
    .unwrap()
}
fn generate(config: &SystemConfig) -> anyhow::Result<String> {
    let configs: BTreeMap<String, BloxConfig> = BTreeMap::new();
    system_wiring::generate(config, &configs, &BTreeMap::new())
}
const SERVICE: &str = r#"
[[services]]
name = "led_io"
impl_crate = "fixture_platform"
start = "start_led"
resource = "led_consumer"
"#;
#[test]
fn mailbox_free_service_has_no_channel_and_freezes_after_submission() {
    let code = generate(&system(SERVICE)).unwrap();
    assert!(code.contains("#![no_std]"));
    assert!(code.contains("#![no_main]"));
    assert!(code.contains("cortex_m_rt::entry"));
    assert!(!code.contains("println!"));
    assert!(!code.contains("led_io_ref"));
    assert!(!code.contains("led_io_rx"));
    assert!(code.find("start_led(").unwrap() < code.find("::freeze()").unwrap());
    assert!(code.contains("resource_led_consumer"));
}
#[test]
fn service_mailbox_fields_are_a_pair() {
    for field in [
        "capacity = 1",
        "message_path = \"fixture_messages::Command\"",
    ] {
        let err = generate(&system(&format!("{SERVICE}\n{field}")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("message_path and capacity"), "{err}");
    }
}
#[test]
fn mailbox_free_service_cannot_be_injected_as_ref() {
    let text = format!(
        r#"{SERVICE}
[[services]]
name = "other"
impl_crate = "fixture_platform"
start = "start_other"
resource = "other"
[services.inject]
output = {{ source = "service", service = "led_io" }}
"#
    );
    let err = generate(&system(&text)).unwrap_err().to_string();
    assert!(err.contains("mailbox-free"), "{err}");
}
#[test]
fn duplicate_owned_resources_fail_before_emission() {
    let second = SERVICE.replace("led_io", "other");
    let err = generate(&system(&format!("{SERVICE}{second}")))
        .unwrap_err()
        .to_string();
    assert!(err.contains("consumed more than once"), "{err}");
}
#[test]
fn embedded_profile_requires_platform_and_target() {
    let mut config = system("");
    config.platform = None;
    assert!(generate(&config)
        .unwrap_err()
        .to_string()
        .contains("platform"));
    let mut config = system("");
    config.system.target = None;
    assert!(generate(&config)
        .unwrap_err()
        .to_string()
        .contains("target"));
}
#[test]
fn typed_service_owns_receiver() {
    let code = generate(&system(&format!(
        "{SERVICE}\ncapacity = 2\nmessage_path = \"fixture_messages::Command\""
    )))
    .unwrap();
    assert!(code.contains("led_io_ref"));
    assert!(code.contains("led_io_rx"));
}

#[test]
fn actor_cannot_receive_mailbox_free_service_ref() {
    let cfg = system(&format!(
        r#"{SERVICE}
[[actors]]
name = "lamp"
blox = "lamp-blox"
[actors.inject]
output = {{ source = "service", service = "led_io" }}
"#
    ));
    assert!(generate(&cfg)
        .unwrap_err()
        .to_string()
        .contains("mailbox-free"));
}
#[test]
fn actor_and_service_cannot_consume_same_resource() {
    let cfg = system(&format!(
        r#"{SERVICE}
[[actors]]
name = "lamp"
blox = "lamp-blox"
[actors.inject]
output = {{ source = "resource", field = "led_consumer" }}
"#
    ));
    assert!(generate(&cfg)
        .unwrap_err()
        .to_string()
        .contains("consumed more than once"));
}
#[test]
fn hosted_features_and_zero_capacity_are_rejected() {
    let mut cfg = system("");
    cfg.system.time_features.push("std".into());
    assert!(generate(&cfg)
        .unwrap_err()
        .to_string()
        .contains("host feature"));
    let cfg = system(&format!(
        "{SERVICE}\ncapacity = 0\nmessage_path = \"fixture_messages::Command\""
    ));
    assert!(generate(&cfg).unwrap_err().to_string().contains("positive"));
}
