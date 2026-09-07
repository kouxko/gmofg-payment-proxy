use std::{path::PathBuf, process::Command};

use bytes::Bytes;
use intercept_proxy_package_runtime::{
    WasmPackageRuntime, embed_package_manifest, read_package_component,
};
use intercept_proxy_product_api::ProductError;

use super::*;

type OutputMutationCase = (&'static str, fn(&mut HttpFinalizeDocument), &'static str);

#[derive(Debug)]
struct Utf8Codec;

impl BodyCodec for Utf8Codec {
    fn id(&self) -> &'static str {
        "test:utf-8"
    }

    fn name(&self) -> &'static str {
        "Test UTF-8"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|error| ProductError::new("BODY_DECODE_FAILED", error.to_string()))
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

#[test]
fn local_finalize_contract_error_keeps_package_direction_and_method() {
    let package = ProtocolPackageRef {
        id: intercept_proxy_domain::ProtocolPackageId::new("http-finalize-contract").unwrap(),
        version: intercept_proxy_domain::ProtocolPackageVersion::new("1.0.0").unwrap(),
    };
    let error = local_finalize_error(
        &package,
        ProtocolDirection::Downstream,
        "downstream-finalize",
        Error::coded("HTTP_FINALIZE_OUTPUT_INVALID", "missing field `code`"),
    );
    let failure = error.external_package_call.unwrap();
    assert_eq!(failure.package, package);
    assert_eq!(failure.direction, ProtocolDirection::Downstream);
    assert_eq!(failure.stage, ExternalPackageCallStage::Finalize);
    assert_eq!(failure.method, "downstream-finalize");
    assert_eq!(
        failure.stable_code.as_deref(),
        Some("HTTP_FINALIZE_OUTPUT_INVALID")
    );
}

#[test]
fn duplicate_json_and_case_insensitive_header_names_use_the_last_value() {
    let document = serde_json::from_str::<HttpFinalizeDocument>(
        r#"{"url":"/","method":"GET","code":null,"header":{"X-A":"one","x-a":"two","X-A":"three"},"body":""}"#,
    )
    .unwrap();
    assert_eq!(
        document.header.normalized().get("x-a").unwrap(),
        &("X-A".into(), "three".into())
    );
}

#[test]
fn unchanged_effective_header_preserves_original_duplicates() {
    let mut message = Message {
        start_line: "HTTP/1.1 200 OK".into(),
        headers: vec![
            RawHeader::new("Set-Cookie", "first=1"),
            RawHeader::new("set-cookie", "last=2"),
        ],
        body: Bytes::new(),
        body_modified: false,
    };
    let before = FinalizeHeaders::from_message(&message).unwrap();
    apply_header_delta(&before, &before, &mut message).unwrap();
    assert_eq!(message.headers.len(), 2);
    assert_eq!(&message.headers[0].value[..], b"first=1");
    assert_eq!(&message.headers[1].value[..], b"last=2");
}

#[test]
fn finalize_output_requires_the_code_key() {
    let error = serde_json::from_str::<HttpFinalizeDocument>(
        r#"{"url":"/","method":"GET","header":{},"body":""}"#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("missing field `code`"));
}

#[test]
fn unchanged_start_line_preserves_version_and_reason_bytes() {
    let input = HttpFinalizeDocument {
        url: "/".into(),
        method: "GET".into(),
        code: Some(299),
        header: FinalizeHeaders::default(),
        body: String::new(),
    };
    let mut message = Message {
        start_line: "HTTP/1.0 299 Custom Reason".into(),
        headers: Vec::new(),
        body: Bytes::new(),
        body_modified: false,
    };
    apply_start_line(ProtocolDirection::Downstream, &input, &input, &mut message).unwrap();
    assert_eq!(message.start_line, "HTTP/1.0 299 Custom Reason");
}

#[test]
fn rejected_finalize_output_never_partially_writes_the_message() {
    let upstream_request = HttpRequestMetadata {
        method: "POST".into(),
        request_target: "/before".into(),
        url: "https://example.test:8443/before".into(),
    };
    let upstream_message = Message {
        start_line: "POST https://example.test:8443/before HTTP/1.1".into(),
        headers: vec![RawHeader::new("X-Value", "before")],
        body: Bytes::from_static(b"before"),
        body_modified: false,
    };
    let upstream_input =
        document_from_message(&Utf8Codec, &upstream_request, &upstream_message).unwrap();
    let upstream_cases: [OutputMutationCase; 4] = [
        (
            "method",
            |output: &mut HttpFinalizeDocument| output.method = "PUT".into(),
            "HTTP_FINALIZE_METHOD_CHANGED",
        ),
        (
            "scheme",
            |output: &mut HttpFinalizeDocument| {
                output.url = "http://example.test:8443/before".into();
            },
            "HTTP_FINALIZE_URL_ORIGIN_CHANGED",
        ),
        (
            "host",
            |output: &mut HttpFinalizeDocument| {
                output.url = "https://other.test:8443/before".into();
            },
            "HTTP_FINALIZE_URL_ORIGIN_CHANGED",
        ),
        (
            "port",
            |output: &mut HttpFinalizeDocument| {
                output.url = "https://example.test:9443/before".into();
            },
            "HTTP_FINALIZE_URL_ORIGIN_CHANGED",
        ),
    ];
    for (name, mutate, code) in upstream_cases {
        let mut output = upstream_input.clone();
        mutate(&mut output);
        assert_rejected_without_write(
            name,
            ProtocolDirection::Upstream,
            &upstream_request,
            &upstream_message,
            &upstream_input,
            &output,
            code,
        );
    }

    let downstream_request = HttpRequestMetadata {
        method: "POST".into(),
        request_target: "/fixed".into(),
        url: "/fixed".into(),
    };
    let downstream_message = Message {
        start_line: "HTTP/1.1 200 OK".into(),
        headers: vec![RawHeader::new("X-Value", "before")],
        body: Bytes::from_static(b"before"),
        body_modified: false,
    };
    let downstream_input =
        document_from_message(&Utf8Codec, &downstream_request, &downstream_message).unwrap();
    let mut cases = Vec::new();
    let mut changed_url = downstream_input.clone();
    changed_url.url = "/changed".into();
    cases.push(("downstream-url", changed_url, "HTTP_FINALIZE_URL_CHANGED"));
    let mut invalid_status = downstream_input.clone();
    invalid_status.code = Some(1000);
    cases.push(("status", invalid_status, "HTTP_FINALIZE_CODE_INVALID"));
    let mut invalid_name = downstream_input.clone();
    invalid_name.header = FinalizeHeaders(vec![("bad header".into(), "value".into())]);
    cases.push(("header-name", invalid_name, "HTTP_FINALIZE_HEADER_INVALID"));
    let mut invalid_value = downstream_input.clone();
    invalid_value.header = FinalizeHeaders(vec![("X-Value".into(), "bad\r\nvalue".into())]);
    cases.push((
        "header-value",
        invalid_value,
        "HTTP_FINALIZE_HEADER_INVALID",
    ));
    for (name, output, code) in cases {
        assert_rejected_without_write(
            name,
            ProtocolDirection::Downstream,
            &downstream_request,
            &downstream_message,
            &downstream_input,
            &output,
            code,
        );
    }
}

fn assert_rejected_without_write(
    name: &str,
    direction: ProtocolDirection,
    request: &HttpRequestMetadata,
    original: &Message,
    input: &HttpFinalizeDocument,
    output: &HttpFinalizeDocument,
    expected_code: &str,
) {
    let mut actual = original.clone();
    let Err(error) = apply_output(direction, &Utf8Codec, request, &mut actual, input, output)
    else {
        panic!("{name} output was accepted");
    };
    assert_eq!(error.code.as_deref(), Some(expected_code), "{name}");
    assert_eq!(actual.start_line, original.start_line, "{name}");
    assert_eq!(actual.headers, original.headers, "{name}");
    assert_eq!(actual.body, original.body, "{name}");
    assert_eq!(actual.body_modified, original.body_modified, "{name}");
}

#[test]
fn malformed_finalize_json_is_rejected() {
    for json in [
        "not-json",
        r#"{"url":[]}"#,
        r#"{"url":"/","method":"GET","code":null,"header":{"X":1},"body":""}"#,
    ] {
        assert!(serde_json::from_str::<HttpFinalizeDocument>(json).is_err());
    }
}

#[tokio::test]
async fn real_wasm_finalize_updates_http_fields() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = manifest_dir.join("../package-runtime/tests/fixtures/http-finalize");
    let target_dir = fixture.join("target/upstream-downstream");
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "--manifest-path",
            fixture.join("Cargo.toml").to_str().unwrap(),
            "--target",
            "wasm32-wasip2",
            "--target-dir",
            target_dir.to_str().unwrap(),
            "--no-default-features",
            "--features",
            "upstream,downstream",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let component = std::fs::read(
        target_dir.join("wasm32-wasip2/debug/intercept_proxy_http_finalize_component.wasm"),
    )
    .unwrap();
    let manifest = std::fs::read(fixture.join("manifest.json")).unwrap();
    let package =
        read_package_component(&embed_package_manifest(&component, &manifest).unwrap()).unwrap();
    let runtime: Arc<dyn ProtocolPackageRuntime> =
        Arc::new(crate::adapters::InProcessWasmRuntime::new(
            WasmPackageRuntime::load(&package).await.unwrap(),
        ));
    let package_ref = package.manifest().package().identity().clone();

    let request = HttpRequestMetadata {
        method: "POST".into(),
        request_target: "/before?value=before".into(),
        url: "https://example.test/before?value=before".into(),
    };
    let mut upstream = Message {
        start_line: "POST https://example.test/before?value=before HTTP/1.1".into(),
        headers: vec![RawHeader::new("X-Value", "before")],
        body: Bytes::from_static(b"before"),
        body_modified: false,
    };
    finalize_http_message(
        &runtime,
        &package_ref,
        ProtocolDirection::Upstream,
        &Utf8Codec,
        &request,
        &mut upstream,
    )
    .await
    .unwrap();
    assert_eq!(
        upstream.start_line,
        "POST https://example.test/upstream?value=upstream HTTP/1.1"
    );
    assert_eq!(&upstream.headers[0].value[..], b"upstream");
    assert_eq!(&upstream.body[..], b"upstream");

    let request = HttpRequestMetadata {
        method: "POST".into(),
        request_target: "/fixed".into(),
        url: "/fixed".into(),
    };
    let mut downstream = Message {
        start_line: "HTTP/1.1 200 OK".into(),
        headers: vec![RawHeader::new("X-Value", "before")],
        body: Bytes::from_static(b"before"),
        body_modified: false,
    };
    finalize_http_message(
        &runtime,
        &package_ref,
        ProtocolDirection::Downstream,
        &Utf8Codec,
        &request,
        &mut downstream,
    )
    .await
    .unwrap();
    assert_eq!(downstream.start_line, "HTTP/1.1 202 Accepted");
    assert_eq!(&downstream.headers[0].value[..], b"downstream");
    assert_eq!(&downstream.body[..], b"downstream");
}
