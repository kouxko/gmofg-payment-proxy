use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

use intercept_proxy_domain::ProtocolDirection;
use intercept_proxy_package_contract::{FrameResult, PackageKind};
use intercept_proxy_package_runtime::{WasmPackageRuntime, read_package_component};
use serde_json::Value;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[tokio::test]
async fn every_repository_example_and_template_builds_and_loads_as_a_component() {
    let repository = repository_root();
    let output = Command::new("node")
        .arg("scripts/build-protocol-package-components.mjs")
        .current_dir(&repository)
        .output()
        .expect("run the unified Component build");
    assert!(
        output.status.success(),
        "unified Component build failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let distribution = repository.join("dist/protocol-package-components");
    let index = serde_json::from_slice::<Value>(
        &std::fs::read(distribution.join("index.json")).expect("read Component build index"),
    )
    .expect("parse Component build index");
    let components = index["components"]
        .as_array()
        .expect("Component build index entries");
    let sources = components
        .iter()
        .map(|entry| entry["source"].as_str().expect("Component source"))
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        "examples/external-packages/au_eftex/component/Cargo.toml",
        "examples/external-packages/iso8583-deno/component/Cargo.toml",
        "examples/external-packages/nuvei_tango_json/component/Cargo.toml",
        "examples/protocol-packages/nuvei_tango_rhai/component/Cargo.toml",
        "templates/socket-protocol/iso8583-standard/Cargo.toml",
    ]);
    assert_eq!(sources, expected, "Component source inventory drifted");

    let package_names = components
        .iter()
        .map(|entry| entry["package"].as_str().expect("Component package name"))
        .collect::<BTreeSet<_>>();
    let artifacts = components
        .iter()
        .map(|entry| entry["artifact"].as_str().expect("Component artifact"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        package_names.len(),
        components.len(),
        "duplicate package name"
    );
    assert_eq!(artifacts.len(), components.len(), "duplicate artifact path");

    let mut identities = BTreeSet::new();
    for entry in components {
        let artifact = entry["artifact"].as_str().expect("artifact path");
        let bytes = std::fs::read(repository.join(artifact)).expect("read built Component");
        let package = read_package_component(&bytes)
            .unwrap_or_else(|error| panic!("validate {artifact} manifest: {error:?}"));
        let package_id = package
            .manifest()
            .package()
            .identity()
            .id
            .as_str()
            .to_owned();
        assert!(!package_id.is_empty());
        let identity = package.manifest().package().identity();
        assert!(
            identities.insert(format!(
                "{}@{}",
                identity.id.as_str(),
                identity.version.as_str()
            )),
            "duplicate embedded package identity in {artifact}"
        );
        assert_eq!(package.manifest().kind(), PackageKind::Socket);
        let mut runtime = WasmPackageRuntime::load(&package)
            .await
            .expect("instantiate built Component world");
        runtime
            .frame(ProtocolDirection::Upstream, &[])
            .await
            .unwrap_or_else(|error| panic!("call {artifact} frame export: {error:?}"));

        match package_id.as_str() {
            "iso8583-deno-ascii" => assert_iso8583_deno_replay(&mut runtime).await,
            "nuvei-tango-json" => assert_nuvei_json_replay(&mut runtime).await,
            "nuvei-tango-json-rhai" => {
                assert_nuvei_rhai_replay(&repository, &mut runtime).await;
            }
            _ => {}
        }
    }
}

async fn assert_iso8583_deno_replay(runtime: &mut WasmPackageRuntime) {
    let frame = decode_hex(
        "0039303230303220000000808000303030303030303030303030303031303030303831333134333035393132333435365445524d30303031333932",
    );
    let document = runtime
        .decode_socket(ProtocolDirection::Upstream, &frame)
        .await
        .expect("decode ISO8583 Deno replay vector");
    let html = runtime
        .display(ProtocolDirection::Upstream, &document)
        .await
        .expect("display ISO8583 Deno Host-normalized Document");
    assert!(html.contains("<td>1000</td>"));
    assert_eq!(
        runtime
            .encode_socket(ProtocolDirection::Upstream, &frame, &document)
            .await
            .expect("encode ISO8583 Deno Host-normalized Document"),
        frame
    );
}

async fn assert_nuvei_json_replay(runtime: &mut WasmPackageRuntime) {
    let frame = decode_hex(
        "0000002c0100010030303030303032307b224163637074724175746873746e526571223a7b2276616c7565223a317d7d",
    );
    let document = runtime
        .decode_socket(ProtocolDirection::Upstream, &frame)
        .await
        .expect("decode Nuvei JSON replay vector");
    let html = runtime
        .display(ProtocolDirection::Upstream, &document)
        .await
        .expect("display Nuvei JSON nested preview");
    assert!(html.contains("<table class=\"protocol-document-nested\">"));
    assert!(html.contains("<th>AccptrAuthstnReq</th>"));
    assert!(html.contains("<th>value</th>"));
    assert!(!html.contains("<pre>"));
    assert_eq!(
        runtime
            .encode_socket(ProtocolDirection::Upstream, &frame, &document)
            .await
            .expect("encode Nuvei JSON replay vector"),
        frame
    );
}

async fn assert_nuvei_rhai_replay(repository: &Path, runtime: &mut WasmPackageRuntime) {
    let payload = std::fs::read(
        repository.join("examples/protocol-packages/nuvei_tango_rhai/tests/fixtures/request.json"),
    )
    .expect("read Nuvei Rhai replay payload");
    let body_bytes = 4 + 8 + payload.len();
    let mut frame = Vec::with_capacity(4 + body_bytes);
    frame.extend_from_slice(
        &u32::try_from(body_bytes)
            .expect("Nuvei Rhai replay body fits u32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    frame.extend_from_slice(b"00000020");
    frame.extend_from_slice(&payload);
    let document = runtime
        .decode_socket(ProtocolDirection::Upstream, &frame)
        .await
        .expect("decode Nuvei Rhai replay vector");
    let html = runtime
        .display(ProtocolDirection::Upstream, &document)
        .await
        .expect("display Nuvei Rhai nested JSON");
    assert!(html.contains("<table class=\"protocol-document-nested\">"));
    assert!(html.contains("<th>AccptrAuthstnReq</th>"));
    assert!(html.contains("<th>PlainCardData</th>"));
    assert_eq!(
        runtime
            .encode_socket(ProtocolDirection::Upstream, &frame, &document)
            .await
            .expect("encode Nuvei Rhai replay vector"),
        frame
    );
}

#[tokio::test]
async fn au_eftex_component_replays_public_old_vectors_without_runtime_bdk_config() {
    let repository = repository_root();
    let output = Command::new("node")
        .arg("scripts/build-protocol-package-components.mjs")
        .current_dir(&repository)
        .env_remove("AU_EFTEX_BDK_FILE")
        .env_remove("AU_EFTEX_BDK_HEX")
        .output()
        .expect("build repository Components without AU EFTEX BDK environment variables");
    assert!(
        output.status.success(),
        "unified Component build failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let artifact =
        repository.join("dist/protocol-package-components/intercept-proxy-au-eftex-component.wasm");
    let package = read_package_component(&std::fs::read(&artifact).expect("read AU EFTEX Wasm"))
        .expect("validate AU EFTEX Wasm");
    assert_eq!(
        package.manifest().package().identity().id.as_str(),
        "au-eftex"
    );
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate AU EFTEX Wasm");

    let vectors = [
        (
            ProtocolDirection::Upstream,
            decode_hex(
                "54df000132df01083132333435363738df0206303030303031df030affff9876543210e00008427b758dda6a29d38b8020b31687b21d636dbc15e6f3a17cdee8a868124d4c8f84",
            ),
        ),
        (
            ProtocolDirection::Downstream,
            decode_hex(
                "54df000132df01083132333435363738df0206303030303031df030affff9876543210e000084247737e0317a4310697a84e728f754c84798309ef10edd18e",
            ),
        ),
    ];
    for (direction, frame) in vectors {
        assert_eq!(
            runtime
                .frame(direction, &frame)
                .await
                .expect("frame old vector"),
            FrameResult::complete(frame.len()).expect("positive old-vector frame length")
        );
        let document = runtime
            .decode_socket(direction, &frame)
            .await
            .expect("decode old vector");
        assert!(
            !runtime
                .display(direction, &document)
                .await
                .expect("display old vector")
                .is_empty()
        );
        assert_eq!(
            runtime
                .encode_socket(direction, &frame, &document)
                .await
                .expect("encode old vector"),
            frame
        );
    }
}

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "hex input must contain full bytes");
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).expect("valid test hex"))
        .collect()
}
