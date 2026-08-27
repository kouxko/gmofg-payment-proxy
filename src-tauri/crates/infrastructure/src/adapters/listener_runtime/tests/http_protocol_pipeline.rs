use std::{
    io::{Cursor, Write},
    sync::Arc,
};

use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{
    DocumentAction, DocumentCondition, DocumentFieldName, DocumentValue, HttpBodyProcessing,
    HttpListenerSettings, ListenerDataPlane, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    ProtocolRuleStage, ProxyListener, ProxyWorkspace,
};
use intercept_proxy_exchange::HttpContext;
use intercept_proxy_runtime::{HttpConnectionIdentity, HttpProtocolCapabilityFactory};
use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{SqliteStore, adapters::ProtocolPackageRepositoryAdapter};

use super::super::http_protocol_pipeline::HttpProtocolRuntimeSnapshot;
use super::test_listener_runtime_with_packages;

const HTTP_MANIFEST: &str = r#"
api = 1

[package]
id = "http-pipeline-test"
name = "HTTP Pipeline Test"
version = "1.0.0"

[document.upstream]
schema = "upstream.toml"
display = "display"

[document.downstream]
schema = "downstream.toml"
display = "display"

[hooks.upstream]
decode = "decode"
encode = "encode"

[hooks.downstream]
decode = "decode"
encode = "encode"
"#;

const UPSTREAM_SCHEMA: &str = r#"
id = "http-upstream"
version = 1
title = "HTTP upstream"

[[fields]]
name = "route"
label = "Route"
type = "string"
"#;

const DOWNSTREAM_SCHEMA: &str = r#"
id = "http-downstream"
version = 1
title = "HTTP downstream"

[[fields]]
name = "result"
label = "Result"
type = "string"
"#;

const PIPELINE_SCRIPT: &str = r#"
fn decode(origin, context) {
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("route", "decoded");
    } else {
        value.set("result", "decoded");
    }
    value
}

fn encode(origin, document, context) {
    if context.direction() == "upstream" {
        origin + ("|" + document.get("route")).to_blob()
    } else {
        origin + ("|" + document.get("result")).to_blob()
    }
}

fn display(document, context) {
    if context.direction() == "upstream" {
        "<p>upstream:" + document.get("route") + "</p>"
    } else {
        "<p>downstream:" + document.get("result") + "</p>"
    }
}
"#;

const DECODE_ONLY_SCRIPT: &str = r#"
fn decode(origin, context) {
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("route", "decoded");
    } else {
        value.set("result", "decoded");
    }
    value
}
fn encode(origin, document, context) { throw "encode invoked"; }
fn display(document, context) { "decoded" }
"#;

const NON_UTF8_ENCODE_SCRIPT: &str = r#"
fn decode(origin, context) {
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("route", "decoded");
    } else {
        value.set("result", "decoded");
    }
    value
}
fn encode(origin, document, context) { blob(1, 255) }
fn display(document, context) { "ok" }
"#;

const DECODE_FAILURE_SCRIPT: &str = r#"
fn decode(origin, context) { throw "decode failed"; }
fn encode(origin, document, context) { origin }
fn display(document, context) { "unused" }
"#;

const DISPLAY_FAILURE_SCRIPT: &str = r#"
fn decode(origin, context) {
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("route", "decoded");
    } else {
        value.set("result", "decoded");
    }
    value
}
fn encode(origin, document, context) { origin }
fn display(document, context) { throw "display failed"; }
"#;

#[tokio::test]
async fn upstream_capabilities_are_independent_and_rules_run_in_order() {
    let listener = http_listener();
    let rules = vec![
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(1)),
            ProtocolRuleStage::AppToProxy,
            1,
            "route",
            Vec::new(),
            "after_app",
        ),
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(2)),
            ProtocolRuleStage::ProxyToUpstream,
            2,
            "route",
            vec![DocumentCondition::Equals {
                field: DocumentFieldName::new("route").unwrap(),
                value: DocumentValue::String("after_app".into()),
            }],
            "after_proxy",
        ),
    ];
    let (snapshot, workspace) = snapshot(PIPELINE_SCRIPT, &listener, rules);
    let metadata = snapshot.observation_metadata();
    assert_eq!(metadata.workspace_id, workspace.id.to_string());
    assert_eq!(metadata.listener_id, listener.id.to_string());

    let mut capabilities = snapshot.create_upstream(identity()).unwrap();
    let original = context("POST /sale HTTP/1.1\r\n\r\n", "wire");
    let document = capabilities.decode.decode(&original).await.unwrap();
    assert_eq!(
        capabilities.display.display(&document).await.unwrap(),
        "<p>upstream:decoded</p>"
    );
    let document = capabilities.rules.apply(document).await.unwrap();
    assert_eq!(
        document.get("route").unwrap(),
        &DocumentValue::String("after_proxy".into())
    );
    let written = capabilities
        .encode
        .encode(&original, &document)
        .await
        .unwrap();
    assert_eq!(written.header, original.header);
    assert_eq!(written.body, "wire|after_proxy");
}

#[tokio::test]
async fn downstream_uses_downstream_schema_and_rule_stages() {
    let listener = http_listener();
    let rules = vec![
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(3)),
            ProtocolRuleStage::UpstreamToProxy,
            1,
            "result",
            Vec::new(),
            "after_server",
        ),
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(4)),
            ProtocolRuleStage::ProxyToApp,
            2,
            "result",
            vec![DocumentCondition::Equals {
                field: DocumentFieldName::new("result").unwrap(),
                value: DocumentValue::String("after_server".into()),
            }],
            "after_proxy",
        ),
    ];
    let (snapshot, _) = snapshot(PIPELINE_SCRIPT, &listener, rules);
    let mut capabilities = snapshot.create_downstream(identity()).unwrap();
    let original = context("HTTP/1.1 200 OK\r\n\r\n", "reply");

    let document = capabilities.decode.decode(&original).await.unwrap();
    let document = capabilities.rules.apply(document).await.unwrap();
    let written = capabilities
        .encode
        .encode(&original, &document)
        .await
        .unwrap();

    assert_eq!(written.header, original.header);
    assert_eq!(written.body, "reply|after_proxy");
}

#[test]
fn http_snapshot_rejects_rule_package_and_schema_drift_below_application() {
    let listener = http_listener();
    let cases = [
        (
            ProtocolPackageRef {
                id: ProtocolPackageId::new("other-http-package").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            1,
        ),
        (http_package(), 2),
    ];

    for (index, (package, schema_version)) in cases.into_iter().enumerate() {
        let rule_id = ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(100 + index as u128));
        let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
            rule_id,
            "invalid runtime binding".into(),
            true,
            10,
            1,
            listener.id,
            package,
            schema_version,
            ProtocolRuleStage::AppToProxy,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap();

        let (result, _) = prepare_snapshot(PIPELINE_SCRIPT, &listener, vec![rule]);
        let error = result.unwrap_err();
        assert_eq!(
            error.view_model.code,
            "DOCUMENT_RULE_RUNTIME_BINDING_MISMATCH"
        );
        assert_eq!(
            error.view_model.entity_id.as_deref(),
            Some(rule_id.to_string().as_str())
        );
    }
}

#[tokio::test]
async fn decode_does_not_invoke_encode() {
    let listener = http_listener();
    let (snapshot, _) = snapshot(DECODE_ONLY_SCRIPT, &listener, Vec::new());
    let mut capabilities = snapshot.create_upstream(identity()).unwrap();

    let document = capabilities
        .decode
        .decode(&context("POST / HTTP/1.1\r\n\r\n", "wire"))
        .await
        .unwrap();

    assert_eq!(
        document.get("route").unwrap(),
        &DocumentValue::String("decoded".into())
    );
}

#[tokio::test]
async fn non_utf8_encode_output_is_rejected_without_mutating_input() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(5)),
        ProtocolRuleStage::AppToProxy,
        1,
        "route",
        Vec::new(),
        "changed",
    );
    let (snapshot, _) = snapshot(NON_UTF8_ENCODE_SCRIPT, &listener, vec![rule]);
    let mut capabilities = snapshot.create_upstream(identity()).unwrap();
    let original = context("POST / HTTP/1.1\r\n\r\n", "wire");
    let document = capabilities.decode.decode(&original).await.unwrap();
    let document = capabilities.rules.apply(document).await.unwrap();

    let error = capabilities
        .encode
        .encode(&original, &document)
        .await
        .unwrap_err();

    assert!(error.message.contains("HTTP_PROTOCOL_OUTPUT_NOT_UTF8"));
    assert_eq!(original.body, "wire");
}

#[tokio::test]
async fn decode_failure_stays_in_decode_capability() {
    let listener = http_listener();
    let (snapshot, _) = snapshot(DECODE_FAILURE_SCRIPT, &listener, Vec::new());
    let mut capabilities = snapshot.create_upstream(identity()).unwrap();

    let error = capabilities
        .decode
        .decode(&context("POST / HTTP/1.1\r\n\r\n", "wire"))
        .await
        .unwrap_err();

    assert!(error.message.starts_with("ENTRY_POINT_FAILED\n"));
}

#[tokio::test]
async fn display_failure_is_returned_for_reader_fallback_policy() {
    let listener = http_listener();
    let (snapshot, _) = snapshot(DISPLAY_FAILURE_SCRIPT, &listener, Vec::new());
    let mut capabilities = snapshot.create_downstream(identity()).unwrap();
    let document = capabilities
        .decode
        .decode(&context("HTTP/1.1 200 OK\r\n\r\n", "reply"))
        .await
        .unwrap();

    let error = capabilities.display.display(&document).await.unwrap_err();

    assert!(error.message.starts_with("ENTRY_POINT_FAILED\n"));
}

fn snapshot(
    script: &str,
    listener: &ProxyListener,
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> (Arc<HttpProtocolRuntimeSnapshot>, ProxyWorkspace) {
    let (result, workspace) = prepare_snapshot(script, listener, rules);
    let snapshot = result.unwrap().expect("HTTP protocol snapshot");
    (snapshot, workspace)
}

fn prepare_snapshot(
    script: &str,
    listener: &ProxyListener,
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> (
    AppResult<Option<Arc<HttpProtocolRuntimeSnapshot>>>,
    ProxyWorkspace,
) {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let packages = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    packages.install_zip(&http_package_zip(script)).unwrap();
    packages.set_enabled(&http_package(), true).unwrap();
    let runtime = test_listener_runtime_with_packages(store, packages);
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        protocol_rule_created_order_high_water: rules
            .iter()
            .map(ProtocolDocumentRuleDefinition::created_order)
            .max()
            .unwrap_or(0),
        protocol_rules: rules,
        ..ProxyWorkspace::default()
    };
    let result = HttpProtocolRuntimeSnapshot::prepare(&runtime, &workspace, listener);
    (result, workspace)
}

fn identity() -> HttpConnectionIdentity {
    HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(10),
        connection_id: Uuid::from_u128(11),
        peer: "127.0.0.1:12345".into(),
    }
}

fn context(header: &str, body: &str) -> HttpContext {
    HttpContext {
        header: header.into(),
        body: body.into(),
        body_is_utf8: true,
    }
}

fn http_listener() -> ProxyListener {
    ProxyListener {
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            body_processing: HttpBodyProcessing::Protocol {
                package: http_package(),
            },
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn set_string_rule(
    listener: &ProxyListener,
    id: ProtocolDocumentRuleId,
    stage: ProtocolRuleStage,
    created_order: u64,
    field: &str,
    conditions: Vec<DocumentCondition>,
    value: &str,
) -> ProtocolDocumentRuleDefinition {
    ProtocolDocumentRuleDefinition::new_named_for_stage(
        id,
        format!("{stage:?}"),
        true,
        10,
        created_order,
        listener.id,
        http_package(),
        1,
        stage,
        conditions,
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new(field).unwrap(),
            value: DocumentValue::String(value.into()),
        }],
    )
    .unwrap()
}

fn http_package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("http-pipeline-test").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn http_package_zip(script: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", HTTP_MANIFEST.as_bytes()),
        ("upstream.toml", UPSTREAM_SCHEMA.as_bytes()),
        ("downstream.toml", DOWNSTREAM_SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
        ("display.rhai", script.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
