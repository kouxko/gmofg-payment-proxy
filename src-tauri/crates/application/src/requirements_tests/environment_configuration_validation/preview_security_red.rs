use super::*;

use crate::requirements_tests::{FakePorts, application_with_fake_ports_and_listener_runtime};
use crate::{
    EnvironmentCandidateEpoch, InMemoryListenerRuntime,
    parse_environment_configuration_candidate_v1,
};

fn candidate_json_with_name(name: &str) -> Vec<u8> {
    let mut candidate: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    candidate["target"]["name"] = serde_json::json!(name);
    serde_json::to_vec(&candidate).unwrap()
}

async fn public_status(candidate_json: &[u8]) -> serde_json::Value {
    let application = application_with_fake_ports_and_listener_runtime(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryListenerRuntime::default()),
    );
    let candidate = parse_environment_configuration_candidate_v1(candidate_json).unwrap();
    let inserted = application
        .environment_candidate_insert_validating(candidate, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let cancellation = CancellationToken::new();
    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate_for_candidate(
            inserted.candidate_id(),
            candidate_json,
            cancellation.clone(),
            cancellation,
            &application,
        )
        .await;
    assert_eq!(report.status_code(), None);
    serde_json::to_value(application.environment_candidate_status(inserted.candidate_id())).unwrap()
}

#[test]
fn preview_builder_never_serializes_the_secret_bearing_candidate() {
    let source = include_str!("../../environment_configuration/preview.rs");

    assert!(
        !source.contains("serde_json::to_value(candidate)"),
        "ordinary preview construction must use typed public projections, not serialize the complete candidate",
    );
}

#[tokio::test]
async fn ordinary_preview_omits_every_private_fixture_value() {
    let candidate: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    let status = public_status(FULL_SHAPE).await;
    let preview = &status["preview"];
    for secret in preview["materials_public"]["secrets"].as_array().unwrap() {
        assert!(secret.get("username").is_none());
        assert!(secret.get("password").is_none());
        assert!(secret.get("content").is_none());
    }
    let mut private_values = candidate["materials"]["certificates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|material| material["content"].as_str())
        .chain(
            candidate["materials"]["certificates"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|material| material["password"].as_str()),
        )
        .chain(
            candidate["materials"]["secrets"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|secret| [secret["username"].as_str(), secret["password"].as_str()])
                .flatten(),
        )
        .collect::<Vec<_>>();
    private_values.sort_unstable();
    private_values.dedup();
    let mut public_strings = Vec::new();
    collect_string_scalars(preview, &mut public_strings);

    for private in private_values {
        assert!(
            !public_strings.contains(&private),
            "ordinary preview exposed a private fixture value",
        );
    }
}

fn collect_string_scalars<'a>(value: &'a serde_json::Value, output: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(value) => output.push(value),
        serde_json::Value::Array(values) => {
            for value in values {
                collect_string_scalars(value, output);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                collect_string_scalars(value, output);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

#[tokio::test]
async fn public_target_keys_preserve_exact_trimmed_utf8_bytes() {
    let cases = [
        ("A", "new:41"),
        ("a", "new:61"),
        ("Ａ", "new:efbca1"),
        ("Café", "new:436166c3a9"),
        ("Cafe\u{301}", "new:43616665cc81"),
    ];
    let mut keys = Vec::new();
    for (name, expected) in cases {
        let status = public_status(&candidate_json_with_name(name)).await;
        assert_eq!(status["target_key"], expected);
        keys.push(status["target_key"].as_str().unwrap().to_owned());
    }
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(
        keys.len(),
        cases.len(),
        "byte-distinct names must not collide"
    );
}

#[test]
fn expected_preview_uses_the_exact_utf8_public_target_key() {
    let expected: serde_json::Value = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/expected-preview.json"
    )))
    .unwrap();

    assert_eq!(expected["target_key"], "new:53746f7265204c6162");
}
