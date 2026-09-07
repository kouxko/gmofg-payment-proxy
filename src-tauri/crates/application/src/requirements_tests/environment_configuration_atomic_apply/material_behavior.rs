use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::{fs, path::Path};

use serde_json::Value;

use super::worker_behavior::queued_registry_from_candidate;
use crate::{
    AppError, AppResult, EnvironmentCommitTarget, EnvironmentPreparedMaterialCapability,
    EnvironmentPreparedMaterialKind, EnvironmentPreparedMaterialVisitor, MaterialAlias,
    parse_environment_configuration_candidate_v1,
};
use zeroize::Zeroizing;

const FULL_SHAPE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json"
));

fn staged(candidate_bytes: &[u8]) -> crate::StagedProtectedMaterialHandle {
    let candidate =
        parse_environment_configuration_candidate_v1(candidate_bytes).expect("candidate parses");
    let (registry, _) = queued_registry_from_candidate(candidate);
    let mut work = registry
        .claim_next_apply()
        .expect("claim")
        .expect("queued work");
    work.take_staged_material().expect("staged material")
}

struct TestPreparedCapability {
    fingerprint: [u8; 32],
}

impl EnvironmentPreparedMaterialCapability for TestPreparedCapability {
    fn consume(
        self: Box<Self>,
        kind: EnvironmentPreparedMaterialKind,
        alias: MaterialAlias,
        visitor: &mut dyn EnvironmentPreparedMaterialVisitor,
    ) -> AppResult<()> {
        visitor.visit(kind, alias, self.fingerprint, Zeroizing::new(Vec::new()))
    }
}

#[derive(Default)]
struct CountingPreparedMaterialVisitor {
    count: usize,
}

impl EnvironmentPreparedMaterialVisitor for CountingPreparedMaterialVisitor {
    fn visit(
        &mut self,
        _kind: EnvironmentPreparedMaterialKind,
        _alias: MaterialAlias,
        _fingerprint: [u8; 32],
        _protected_payload: Zeroizing<Vec<u8>>,
    ) -> AppResult<()> {
        self.count += 1;
        Ok(())
    }
}

#[test]
fn staged_candidate_lends_each_typed_material_record_to_the_protector_once() {
    let protected_inputs = Arc::new(Mutex::new(Vec::new()));
    let observed = protected_inputs.clone();

    let prepared = staged(FULL_SHAPE)
        .prepare_with(move |plaintext, _, fingerprint| {
            observed.lock().unwrap().push(plaintext.to_vec());
            Ok(Box::new(TestPreparedCapability { fingerprint })
                as Box<dyn EnvironmentPreparedMaterialCapability>)
        })
        .expect("prepare");

    let inputs = protected_inputs.lock().unwrap();
    let mut visitor = CountingPreparedMaterialVisitor::default();
    let prepared = prepared.consume_with(&mut visitor).expect("consume");
    match &prepared.target {
        EnvironmentCommitTarget::New {
            workspace_id,
            display_name,
        } => {
            assert!(!workspace_id.is_nil());
            assert_eq!(*workspace_id, prepared.workspace.id.as_uuid());
            assert_eq!(display_name, "Store Lab");
        }
        EnvironmentCommitTarget::Existing { .. } => panic!("fixture is a new target"),
    }
    assert_eq!(inputs.len(), visitor.count);
    for input in inputs.iter() {
        let record: Value = serde_json::from_slice(input).expect("typed material record JSON");
        assert!(record.get("alias").and_then(Value::as_str).is_some());
        assert!(record.get("workspace").is_none());
        assert!(record.get("target").is_none());
    }
}

#[test]
fn protector_failure_stops_preparation_and_preserves_stable_error() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();

    let Err(error) = staged(FULL_SHAPE).prepare_with(move |_, _, _| {
        observed.fetch_add(1, Ordering::SeqCst);
        Err::<Box<dyn EnvironmentPreparedMaterialCapability>, _>(AppError::new(
            "PROTECTED_MATERIAL_PREPARE_FAILED",
            "protector failed",
        ))
    }) else {
        panic!("protector failure must propagate");
    };

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(error.view_model.code, "PROTECTED_MATERIAL_PREPARE_FAILED");
}

#[test]
fn certificate_and_secret_alias_collision_rejects_before_secret_protection() {
    let mut candidate: Value = serde_json::from_slice(FULL_SHAPE).expect("fixture");
    let certificate_alias = candidate["materials"]["certificates"][0]["alias"]
        .as_str()
        .expect("certificate alias")
        .to_owned();
    candidate["materials"]["secrets"][0]["alias"] = Value::String(certificate_alias);
    let encoded = serde_json::to_vec(&candidate).expect("candidate JSON");
    let certificate_count = candidate["materials"]["certificates"]
        .as_array()
        .expect("certificates")
        .len();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();

    let Err(error) = staged(&encoded).prepare_with(move |_, _, fingerprint| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(TestPreparedCapability { fingerprint })
            as Box<dyn EnvironmentPreparedMaterialCapability>)
    }) else {
        panic!("cross-family alias collision must reject");
    };

    assert_eq!(error.view_model.code, "MATERIAL_ALIAS_DUPLICATE");
    assert_eq!(calls.load(Ordering::SeqCst), certificate_count);
}

#[test]
fn application_batch_handle_has_no_public_generic_payload_escape_hatch() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/environment_configuration/apply.rs"),
    )
    .expect("apply source");

    for forbidden in ["pub fn erase<", "pub fn downcast<", "Box<dyn std::any::Any"] {
        assert!(
            !source.contains(forbidden),
            "Application exposes prepared material payload through `{forbidden}`"
        );
    }
}

#[test]
fn application_cannot_construct_or_extract_prepared_batch_capability_ids() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/environment_configuration/apply.rs"),
    )
    .expect("apply source");

    for forbidden in [
        "pub fn from_infrastructure(",
        "pub fn authorize(",
        "pub fn seal(",
        "pub fn claim_id(",
        "pub fn pending(",
        "pub fn from_batch(",
        "pub fn batch(",
        "pub fn empty_marker(",
        "pub fn seal_batch(",
        "pub fn sealed_batch(",
        "Box<dyn std::any::Any",
        "pub fn downcast<",
        "pub fn bind(",
        "pub fn entry_for(",
        "pub fn marker(",
        "pub fn attach(",
        "pub fn attached(",
    ] {
        assert!(
            !source.contains(forbidden),
            "Application can forge or extract a prepared capability through `{forbidden}`"
        );
    }

    for path in ["src/environment_configuration/mod.rs", "src/lib.rs"] {
        let boundary = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
            .expect("Application boundary source");
        assert!(
            !boundary.contains("PreparedMaterialBatchHandle"),
            "opaque Infrastructure batch handle is re-exported through `{path}`"
        );
    }
}
