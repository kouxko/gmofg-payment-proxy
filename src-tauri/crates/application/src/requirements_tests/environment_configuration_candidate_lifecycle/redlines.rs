use super::support::*;

#[test]
fn typed_preview_create_and_status_exclude_private_fixture_material() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Store Lab", 1);
    let create_json = serde_json::to_string(&ready).expect("create output serializes");
    let status_json = serde_json::to_string(&registry.status(ready.candidate_id()))
        .expect("status output serializes");

    for private_value in ["fixture-password", "-----BEGIN CERTIFICATE-----", "AQID"] {
        assert!(!create_json.contains(private_value));
        assert!(!status_json.contains(private_value));
    }
    assert!(create_json.contains("HTTP listener identity"));
    assert!(create_json.contains("proxy-admin"));
}

#[test]
fn registry_and_apply_guard_debug_text_exclude_private_fixture_material() {
    let registry = registry();
    let (_ready, work) = claim_apply(&registry, "Debug Guard");
    let debug_text = format!("{registry:?} {work:?}");

    for private_value in ["fixture-password", "-----BEGIN CERTIFICATE-----", "AQID"] {
        assert!(!debug_text.contains(private_value));
    }
}

#[test]
fn token_debug_text_is_redacted() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Token Debug", 1);
    let token = token_from_create(&ready);
    let raw = json(&ready)["confirmation_token"]
        .as_str()
        .expect("create exposes token")
        .to_owned();

    assert!(!format!("{token:?}").contains(&raw));
}

#[test]
fn terminal_cleanup_removes_all_private_candidate_bytes() {
    let registry = registry();
    let admitted = insert_validating(&registry, "Zero Private", 1);
    assert!(registry.metrics().private_candidate_bytes() > 0);

    registry.cancel(admitted.candidate_id());

    assert_eq!(registry.metrics().private_candidate_bytes(), 0);
}

#[test]
fn validation_failure_diagnostics_status_and_debug_never_echo_candidate_secrets() {
    let registry = registry();
    let admitted = insert_validating(&registry, "Diagnostic Redline", 1);

    fail_validation(&registry, admitted.candidate_id())
        .expect("registered diagnostic code terminalizes validation");
    let status = registry.status(admitted.candidate_id());
    let serialized = serde_json::to_string(&status).expect("safe status serializes");
    let debug_text = format!("{status:?} {registry:?}");

    for private_value in ["fixture-password", "-----BEGIN CERTIFICATE-----", "AQID"] {
        assert!(!serialized.contains(private_value));
        assert!(!debug_text.contains(private_value));
    }
    assert!(serialized.contains("environment validation layer failed"));
}
