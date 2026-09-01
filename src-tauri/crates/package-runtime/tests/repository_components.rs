use std::{collections::BTreeSet, path::PathBuf, process::Command};

use intercept_proxy_domain::ProtocolDirection;
use intercept_proxy_package_contract::PackageKind;
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
        assert!(
            !package
                .manifest()
                .package()
                .identity()
                .id
                .as_str()
                .is_empty()
        );
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
    }
}
