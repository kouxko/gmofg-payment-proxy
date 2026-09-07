use std::{path::PathBuf, process::Command};

use intercept_proxy_domain::{ErrorCode, ProtocolDirection};
use intercept_proxy_package_runtime::{
    WasmPackageRuntime, embed_package_manifest, read_package_component,
};
use wasm_encoder::{
    Component, ComponentExportKind, ComponentExportSection, ComponentInstanceSection,
};

fn build_http_finalize_fixture(features: &[&str]) -> Vec<u8> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = manifest_dir.join("tests/fixtures/http-finalize");
    let feature_key = if features.is_empty() {
        "none".to_owned()
    } else {
        features.join("-")
    };
    let target_dir = fixture.join("target").join(feature_key);
    let mut command = Command::new(env!("CARGO"));
    command.args([
        "build",
        "--locked",
        "--manifest-path",
        fixture.join("Cargo.toml").to_str().expect("UTF-8 path"),
        "--target",
        "wasm32-wasip2",
        "--target-dir",
        target_dir.to_str().expect("UTF-8 path"),
        "--no-default-features",
    ]);
    if !features.is_empty() {
        command.args(["--features", &features.join(",")]);
    }
    let output = command.output().expect("build HTTP Finalize fixture");
    assert!(
        output.status.success(),
        "fixture build failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let component = std::fs::read(
        target_dir.join("wasm32-wasip2/debug/intercept_proxy_http_finalize_component.wasm"),
    )
    .expect("read built HTTP Finalize Component fixture");
    let manifest = std::fs::read(fixture.join("manifest.json")).expect("read HTTP manifest");
    embed_package_manifest(&component, &manifest).expect("embed top-level HTTP manifest")
}

#[tokio::test]
async fn loads_a_committed_legacy_base_world_component_without_finalize() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let bytes = std::fs::read(
        repository.join("examples/protocol-packages/json_pretty/dist/json-pretty-1.0.0.wasm"),
    )
    .expect("read committed legacy HTTP Component");
    let package = read_package_component(&bytes).expect("validate legacy HTTP Component");
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("load legacy base HTTP world");

    assert!(!runtime.has_http_finalize(ProtocolDirection::Upstream));
    assert!(!runtime.has_http_finalize(ProtocolDirection::Downstream));
    assert_eq!(
        runtime
            .finalize_http(ProtocolDirection::Upstream, "unchanged")
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn discovers_and_calls_independent_optional_http_finalize_exports() {
    let cases: &[(&[&str], bool, bool)] = &[
        (&[], false, false),
        (&["upstream"], true, false),
        (&["downstream"], false, true),
        (&["upstream", "downstream"], true, true),
    ];

    for &(features, has_upstream, has_downstream) in cases {
        let package = read_package_component(&build_http_finalize_fixture(features))
            .expect("valid HTTP Finalize Component");
        let mut runtime = WasmPackageRuntime::load(&package)
            .await
            .expect("load HTTP Finalize Component");

        assert_eq!(
            runtime.has_http_finalize(ProtocolDirection::Upstream),
            has_upstream
        );
        assert_eq!(
            runtime.has_http_finalize(ProtocolDirection::Downstream),
            has_downstream
        );

        let upstream = runtime
            .finalize_http(ProtocolDirection::Upstream, "before")
            .await
            .expect("call optional upstream Finalize");
        assert_eq!(upstream, has_upstream.then(|| "upstream".to_owned()));
        let downstream = runtime
            .finalize_http(ProtocolDirection::Downstream, "before")
            .await
            .expect("call optional downstream Finalize");
        assert_eq!(downstream, has_downstream.then(|| "downstream".to_owned()));

        if has_downstream {
            let repeated = runtime
                .finalize_http(
                    ProtocolDirection::Downstream,
                    downstream.as_deref().expect("downstream result"),
                )
                .await
                .expect("repeat downstream Finalize after post-return");
            assert_eq!(repeated.as_deref(), Some("downstream"));
        }
    }
}

#[tokio::test]
async fn preserves_finalize_guest_errors_and_traps() {
    let package = read_package_component(&build_http_finalize_fixture(&["downstream"]))
        .expect("valid downstream HTTP Finalize Component");
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("load downstream HTTP Finalize Component");

    let guest_error = runtime
        .finalize_http(ProtocolDirection::Downstream, "guest-error")
        .await
        .expect_err("guest error must propagate");
    assert_eq!(guest_error.code, ErrorCode::BodyEncodeFailed);
    assert_eq!(
        guest_error.message,
        "downstream finalize rejected the HTTP JSON"
    );

    let trap = runtime
        .finalize_http(ProtocolDirection::Downstream, "trap")
        .await
        .expect_err("guest trap must propagate");
    assert_eq!(trap.code, ErrorCode::InternalError);
    assert!(trap.message.starts_with("Wasm package trapped:"));
}

#[tokio::test]
async fn rejects_present_http_finalize_export_with_the_wrong_signature() {
    let package = read_package_component(&build_http_finalize_fixture(&["invalid-signature"]))
        .expect("statically valid Component");
    let error = WasmPackageRuntime::load(&package)
        .await
        .expect_err("wrong Finalize signature must invalidate the package");
    assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid);
    assert!(error.message.contains("downstream-finalize"));
    assert!(error.message.contains("invalid signature"));
}

#[tokio::test]
async fn rejects_present_http_finalize_export_that_is_not_a_function() {
    let mut instances = ComponentInstanceSection::new();
    instances.export_items(std::iter::empty::<(&str, ComponentExportKind, u32)>());
    let mut exports = ComponentExportSection::new();
    exports.export(
        "downstream-finalize",
        ComponentExportKind::Instance,
        0,
        None,
    );
    let mut component = Component::new();
    component.section(&instances);
    component.section(&exports);
    let manifest = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/http-finalize/manifest.json"),
    )
    .unwrap();
    let bytes = embed_package_manifest(&component.finish(), &manifest).unwrap();
    let package = read_package_component(&bytes).expect("statically valid Component");
    let error = WasmPackageRuntime::load(&package)
        .await
        .expect_err("non-function Finalize export must invalidate the package");
    assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid);
    assert!(error.message.contains("downstream-finalize"));
    assert!(error.message.contains("is not a function"));
}
