use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::SystemTime,
};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, BreakpointCoordinator, EventHub, InMemorySessionStore,
};
use intercept_proxy_domain::{
    ChannelId, DocumentValue, HttpAction, HttpBodyProcessing, HttpListenerSettings,
    HttpRuleContent, JsonPointer, ListenerDataPlane, MatchContext, MatchField, MatchOperator,
    MessageStage, ProtocolDocumentOperation, ProtocolDocumentPredicate,
    ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ProtocolRuleStage, ProxyListener, ProxyWorkspace, RuleContent,
    RuleDefinition, RuleDefinitionDraft, RuleEngine, RuleEvaluation, RuntimeEpoch,
    SocketRuleContent, TerminalIdentity,
};
use intercept_proxy_exchange::HttpContext;
use intercept_proxy_product_api::{
    BodyCodec, ClassifiedRequest, ProductMessageContext, RequestClassifier,
};
use intercept_proxy_runtime::{
    ConnectionContext, HttpConnectionIdentity, HttpProtocolCapabilityFactory, Message,
    PipelinePorts,
};
use parking_lot::Mutex;
use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    SqliteStore,
    adapters::{
        CaptureRepositoryAdapter, ProtocolPackageRepositoryAdapter, RuntimePipelineAdapter,
        RuntimePipelineProductHooks, pipeline::RuntimeRuleRepository,
    },
};

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
type = "object"
title = "HTTP upstream"

[properties.route]
type = "string"
title = "Route"
"#;

const DOWNSTREAM_SCHEMA: &str = r#"
type = "object"
title = "HTTP downstream"

[properties.result]
type = "string"
title = "Result"
"#;

const PIPELINE_SCRIPT: &str = r#"
fn decode(origin, context) {
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("/route", "decoded");
    } else {
        value.set("/result", "decoded");
    }
    value
}

fn encode(origin, document, context) {
    if context.direction() == "upstream" {
        origin + ("|" + document.get("/route")).to_blob()
    } else {
        origin + ("|" + document.get("/result")).to_blob()
    }
}

fn display(document, context) {
    if context.direction() == "upstream" {
        "<p>upstream:" + document.get("/route") + "</p>"
    } else {
        "<p>downstream:" + document.get("/result") + "</p>"
    }
}
"#;

const DECODE_ONLY_SCRIPT: &str = r#"
fn decode(origin, context) {
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("/route", "decoded");
    } else {
        value.set("/result", "decoded");
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
        value.set("/route", "decoded");
    } else {
        value.set("/result", "decoded");
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
        value.set("/route", "decoded");
    } else {
        value.set("/result", "decoded");
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
            ProtocolRuleStage::ProxyToUpstream,
            1,
            "route",
            vec![ProtocolDocumentPredicate::Equals {
                field: JsonPointer::property("route"),
                value: DocumentValue::String("decoded".into()),
            }],
            "after_app",
        ),
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(2)),
            ProtocolRuleStage::ProxyToUpstream,
            2,
            "route",
            vec![ProtocolDocumentPredicate::Equals {
                field: JsonPointer::property("route"),
                value: DocumentValue::String("after_app".into()),
            }],
            "after_proxy",
        ),
    ];
    let (snapshot, workspace) = snapshot(PIPELINE_SCRIPT, &listener, rules);
    let metadata = snapshot.observation_metadata();
    assert_eq!(metadata.workspace_id, workspace.id.to_string());
    assert_eq!(metadata.listener_id, listener.id.to_string());

    let identity = identity();
    let mut capabilities = snapshot.create_upstream(identity.clone()).unwrap();
    let original = context("POST /sale HTTP/1.1\r\n\r\n", "wire");
    let document = capabilities.decode.decode(&original).await.unwrap();
    assert_eq!(
        capabilities.display.display(&document).await.unwrap(),
        "<p>upstream:decoded</p>"
    );
    let document = capabilities.rules.apply(document).await.unwrap();
    assert_eq!(
        document.resolve(&JsonPointer::property("route")).unwrap(),
        &DocumentValue::String("decoded".into())
    );
    let (written, _) = execute_joint(&snapshot, &workspace, &identity, false, &original)
        .await
        .expect("joint upstream execution");
    assert_eq!(written.body, Bytes::from_static(b"wire|after_proxy"));
}

#[tokio::test]
async fn earlier_rule_document_actions_are_visible_to_later_rule_conditions() {
    let listener = http_listener();
    let rules = vec![
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(21)),
            ProtocolRuleStage::ProxyToUpstream,
            1,
            "route",
            Vec::new(),
            "stage-one",
        ),
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(22)),
            ProtocolRuleStage::ProxyToUpstream,
            2,
            "route",
            vec![ProtocolDocumentPredicate::Equals {
                field: JsonPointer::property("route"),
                value: DocumentValue::String("stage-one".into()),
            }],
            "stage-two",
        ),
    ];
    let (snapshot, mut workspace) = snapshot(PIPELINE_SCRIPT, &listener, rules);
    let first = workspace
        .rule_definitions
        .iter_mut()
        .find(|rule| rule.created_order() == 1)
        .expect("first ordered rule");
    let mut draft = first.to_draft();
    draft.priority = 100;
    first.update(first.revision(), draft).unwrap();
    let second = workspace
        .rule_definitions
        .iter_mut()
        .find(|rule| rule.stage() == intercept_proxy_domain::RuleStage::ProxyToUpstream)
        .expect("second stage rule");
    let mut draft = second.to_draft();
    draft.priority = 0;
    let RuleContent::Http(content) = &mut draft.content else {
        panic!("HTTP rule expected");
    };
    content
        .actions
        .push(intercept_proxy_domain::UnifiedAction::from(
            HttpAction::SetHeader {
                name: "x-stage-two".into(),
                value: "matched".into(),
            },
        ));
    second.update(second.revision(), draft).unwrap();

    let identity = identity();
    let mut capabilities = snapshot.create_upstream(identity.clone()).unwrap();
    let original = context("POST /sale HTTP/1.1\r\n\r\n", "wire");
    let document = capabilities.decode.decode(&original).await.unwrap();
    capabilities.rules.apply(document).await.unwrap();
    let (written, evaluation) = execute_joint(&snapshot, &workspace, &identity, false, &original)
        .await
        .expect("joint execution");

    assert_eq!(written.body, Bytes::from_static(b"wire|stage-two"));
    assert_eq!(
        evaluation
            .traces
            .iter()
            .filter(|trace| trace.matched)
            .count(),
        2
    );
}

#[tokio::test]
async fn downstream_uses_downstream_schema_and_rule_stages() {
    let listener = http_listener();
    let rules = vec![
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(3)),
            ProtocolRuleStage::ProxyToApp,
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
            vec![ProtocolDocumentPredicate::Equals {
                field: JsonPointer::property("result"),
                value: DocumentValue::String("after_server".into()),
            }],
            "after_proxy",
        ),
    ];
    let (snapshot, workspace) = snapshot(PIPELINE_SCRIPT, &listener, rules);
    let identity = identity();
    let mut capabilities = snapshot.create_downstream(identity.clone()).unwrap();
    let original = context("HTTP/1.1 200 OK\r\n\r\n", "reply");

    let document = capabilities.decode.decode(&original).await.unwrap();
    let _document = capabilities.rules.apply(document).await.unwrap();
    let (written, _) = execute_joint(&snapshot, &workspace, &identity, true, &original)
        .await
        .expect("joint downstream execution");
    assert_eq!(written.body, Bytes::from_static(b"reply|after_proxy"));
}

#[test]
fn http_snapshot_rejects_rule_package_drift_below_application() {
    let listener = http_listener();
    let cases = [ProtocolPackageRef {
        id: ProtocolPackageId::new("other-http-package").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }];

    for (index, package) in cases.into_iter().enumerate() {
        let rule_id = ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(100 + index as u128));
        let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
            rule_id,
            "invalid runtime binding".into(),
            true,
            10,
            1,
            listener.id,
            package,
            ProtocolRuleStage::ProxyToUpstream,
            vec![ProtocolDocumentPredicate::Equals {
                field: JsonPointer::property("route"),
                value: DocumentValue::String("decoded".into()),
            }],
            vec![ProtocolDocumentOperation::RecordMatch],
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
        document.resolve(&JsonPointer::property("route")).unwrap(),
        &DocumentValue::String("decoded".into())
    );
}

#[tokio::test]
async fn non_utf8_encode_output_is_rejected_without_mutating_input() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(5)),
        ProtocolRuleStage::ProxyToUpstream,
        1,
        "route",
        Vec::new(),
        "changed",
    );
    let (snapshot, workspace) = snapshot(NON_UTF8_ENCODE_SCRIPT, &listener, vec![rule]);
    let identity = identity();
    let mut capabilities = snapshot.create_upstream(identity.clone()).unwrap();
    let original = context("POST / HTTP/1.1\r\n\r\n", "wire");
    let document = capabilities.decode.decode(&original).await.unwrap();
    let _document = capabilities.rules.apply(document).await.unwrap();

    let error = execute_joint(&snapshot, &workspace, &identity, false, &original)
        .await
        .unwrap_err();

    assert!(error.contains("HTTP_PROTOCOL_OUTPUT_NOT_UTF8"));
    assert_eq!(original.body, "wire");
}

include!("http_protocol_pipeline/joint_atomic.rs");
include!("http_protocol_pipeline/support_and_contract.rs");
