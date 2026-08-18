use std::{
    io::{Cursor, Write},
    net::SocketAddr,
    sync::Arc,
    time::UNIX_EPOCH,
};

use bytes::Bytes;
use intercept_proxy_application::{
    HttpProtocolBodyViewModel, HttpProtocolDisplayFallbackReason, HttpProtocolDisplayViewModel,
    HttpProtocolFailureKind, HttpProtocolFailureViewModel,
};
use intercept_proxy_domain::{
    DocumentAction, DocumentCondition, DocumentFieldName, DocumentValue, HttpBodyProcessing,
    HttpListenerSettings, ListenerDataPlane, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    ProtocolRuleStage, ProxyListener, ProxyWorkspace,
};
use intercept_proxy_protocol_scripting::ProtocolDirection;
use intercept_proxy_runtime::{
    ChannelId, ConnectionContext, ErrorCode, HandshakePolicy, Message, NoopPipelinePorts,
    PipelinePorts, RawHeader,
};
use parking_lot::Mutex;
use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{ProtocolPackageRepositoryAdapter, SqliteStore};

use super::super::http_protocol_pipeline::{
    HttpProtocolObservationSink, HttpProtocolRuntimeSnapshot,
};
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

const ENCODE_MUST_NOT_RUN_SCRIPT: &str = r#"
fn decode(origin, context) {
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("route", "decoded");
    } else {
        value.set("result", "decoded");
    }
    value
}

fn encode(origin, document, context) { throw "encode must not run"; }

fn display(document, context) {
    if context.direction() == "upstream" {
        "<p>upstream:" + document.get("route") + "</p>"
    } else {
        "<p>downstream:" + document.get("result") + "</p>"
    }
}
"#;

const DECODE_FAILURE_SCRIPT: &str = r#"
fn decode(origin, context) { throw "decode failed"; }
fn encode(origin, document, context) { origin }
fn display(document, context) { "<p>unused</p>" }
"#;

const ENCODE_FAILURE_SCRIPT: &str = r#"
fn decode(origin, context) {
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("route", "decoded");
    } else {
        value.set("result", "decoded");
    }
    value
}
fn encode(origin, document, context) { throw "encode failed"; }
fn display(document, context) { "<p>ok</p>" }
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
fn display(document, context) { "<p>ok</p>" }
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
fn encode(origin, document, context) {
    if context.direction() == "upstream" {
        origin + ("|" + document.get("route")).to_blob()
    } else {
        origin + ("|" + document.get("result")).to_blob()
    }
}
fn display(document, context) { throw "display failed"; }
"#;

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecordedHttpObservation {
    direction: ProtocolDirection,
    final_body: Bytes,
    observation: HttpProtocolBodyViewModel,
}

#[derive(Debug, Default)]
struct RecordingHttpObservationSink {
    records: Mutex<Vec<RecordedHttpObservation>>,
    failures: Mutex<Vec<HttpProtocolFailureViewModel>>,
}

#[derive(Debug)]
struct RewritingHttpPipeline {
    request_body: Bytes,
    response_body: Bytes,
}

impl HandshakePolicy for RewritingHttpPipeline {}

#[async_trait::async_trait]
impl PipelinePorts for RewritingHttpPipeline {
    async fn request(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> intercept_proxy_runtime::Result<Vec<intercept_proxy_runtime::FaultAction>> {
        message.replace_body(self.request_body.clone());
        Ok(Vec::new())
    }

    async fn response(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> intercept_proxy_runtime::Result<Vec<intercept_proxy_runtime::FaultAction>> {
        message.replace_body(self.response_body.clone());
        Ok(Vec::new())
    }
}

impl RecordingHttpObservationSink {
    fn records(&self) -> Vec<RecordedHttpObservation> {
        self.records.lock().clone()
    }

    fn failures(&self) -> Vec<HttpProtocolFailureViewModel> {
        self.failures.lock().clone()
    }
}

impl HttpProtocolObservationSink for RecordingHttpObservationSink {
    fn record_http_protocol_observation(
        &self,
        _context: &ConnectionContext,
        direction: ProtocolDirection,
        message: &Message,
        observation: HttpProtocolBodyViewModel,
    ) -> intercept_proxy_runtime::Result<()> {
        self.records.lock().push(RecordedHttpObservation {
            direction,
            final_body: message.body.clone(),
            observation,
        });
        Ok(())
    }

    fn record_http_protocol_failure(
        &self,
        _context: &ConnectionContext,
        _message: &Message,
        failure: HttpProtocolFailureViewModel,
    ) -> intercept_proxy_runtime::Result<()> {
        self.failures.lock().push(failure);
        Ok(())
    }
}

#[path = "http_protocol_pipeline/failures.rs"]
mod failures;
#[path = "http_protocol_pipeline/success.rs"]
mod success;

fn pipeline(
    script: &str,
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> (
    Arc<dyn PipelinePorts>,
    Arc<RecordingHttpObservationSink>,
    ProxyListener,
) {
    let listener = http_listener();
    let (pipeline, observations) = pipeline_for_listener(script, &listener, rules);
    (pipeline, observations, listener)
}

fn pipeline_for_listener(
    script: &str,
    listener: &ProxyListener,
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> (Arc<dyn PipelinePorts>, Arc<RecordingHttpObservationSink>) {
    pipeline_for_listener_with_inner(script, listener, rules, Arc::new(NoopPipelinePorts))
}

fn pipeline_for_listener_with_inner(
    script: &str,
    listener: &ProxyListener,
    rules: Vec<ProtocolDocumentRuleDefinition>,
    inner: Arc<dyn PipelinePorts>,
) -> (Arc<dyn PipelinePorts>, Arc<RecordingHttpObservationSink>) {
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
    let snapshot = HttpProtocolRuntimeSnapshot::prepare(&runtime, &workspace, listener)
        .unwrap()
        .expect("HTTP protocol snapshot");
    let observations = Arc::new(RecordingHttpObservationSink::default());
    let pipeline = snapshot.wrap(
        inner,
        Arc::clone(&observations) as Arc<dyn HttpProtocolObservationSink>,
    );
    (pipeline, observations)
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

fn request_message(body: Bytes) -> Message {
    Message {
        start_line: "POST /pay HTTP/1.1".into(),
        headers: vec![RawHeader::new(
            Bytes::from_static(b"Content-Length"),
            Bytes::from(body.len().to_string()),
        )],
        body,
        body_modified: false,
    }
}

fn response_message(body: Bytes) -> Message {
    Message {
        start_line: "HTTP/1.1 200 OK".into(),
        headers: vec![RawHeader::new(
            Bytes::from_static(b"Content-Length"),
            Bytes::from(body.len().to_string()),
        )],
        body,
        body_modified: false,
    }
}

fn test_http_context() -> ConnectionContext {
    ConnectionContext {
        runtime_epoch: Uuid::from_u128(10),
        connection_id: Uuid::from_u128(11),
        channel: ChannelId::new("http-test").unwrap(),
        peer_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        accepted_at: UNIX_EPOCH,
        tls_peer: None,
    }
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
