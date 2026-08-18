use std::{io::Write, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_application::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, ApplicationConfigurationDocument,
    PortableApplicationProtocolPackage, PortableSettings, ProtocolPackagePortabilityPort,
    SettingsDraft,
};
use intercept_proxy_domain::{
    DirectionProcessingOptions, ListenerDataPlane, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ProxyListener, ProxyWorkspace, ScriptedSocketProcessing,
    SocketEndpoint, SocketPayloadProcessing, SocketRelaySecurity, SocketRelaySettings,
};
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::SqliteStore;

use super::*;

const MANIFEST: &str = r#"
api = 1

[package]
id = "portable-test"
name = "Portable Test"
version = "1.0.0"

[document]
schema = "document.toml"

[hooks.upstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"

[hooks.downstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"
"#;

const SCHEMA: &str = r#"
id = "portable-message"
version = 1
title = "Portable Message"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;

const SCRIPT: &str = r"
fn frame(reader, context) { framing::complete(1) }
fn decode(origin, context) { document::create() }
";

#[tokio::test]
async fn application_bundle_restores_enabled_removes_extra_and_clears_cache() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository.install_zip(&package_zip(SCRIPT)).unwrap();
    let second_manifest = MANIFEST.replace("1.0.0", "2.0.0");
    repository
        .install_zip(&package_zip_with_manifest(&second_manifest, SCRIPT))
        .unwrap();
    assert!(repository.compiled(&package()).is_ok());
    let portable = repository
        .export_application_packages()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.package == package())
        .unwrap();
    let imported = PortableApplicationProtocolPackage {
        enabled: true,
        ..portable
    };
    let workspace = workspace_with_package();
    let document = application_document(workspace, vec![imported.clone()]);

    repository
        .replace_application_bundle(vec![imported], document)
        .await
        .unwrap();

    assert_eq!(repository.list().unwrap().len(), 1);
    assert!(repository.summary(&package()).unwrap().unwrap().enabled);
    assert!(repository.cache.lock().is_empty());
}

#[tokio::test]
async fn invalid_last_application_package_causes_zero_writes() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository.install_zip(&package_zip(SCRIPT)).unwrap();
    let original = repository.export_application_packages().await.unwrap();
    let mut invalid = original[0].clone();
    invalid.package.version = ProtocolPackageVersion::new("9.0.0").unwrap();
    invalid
        .files
        .iter_mut()
        .find(|file| file.path == "manifest.toml")
        .unwrap()
        .contents_base64 = STANDARD.encode(MANIFEST.replace("1.0.0", "9.0.0"));
    invalid
        .files
        .iter_mut()
        .find(|file| file.path == "protocol.rhai")
        .unwrap()
        .contents_base64 = STANDARD.encode("fn frame( {");
    let packages = vec![original[0].clone(), invalid];
    let workspace = workspace_with_package();
    let document = application_document(workspace, packages.clone());
    let before_packages = repository.list().unwrap();
    let before_workspaces = store.load_workspaces().unwrap();

    assert!(
        repository
            .replace_application_bundle(packages, document)
            .await
            .is_err()
    );
    assert_eq!(repository.list().unwrap(), before_packages);
    assert_eq!(store.load_workspaces().unwrap(), before_workspaces);
}

#[tokio::test]
async fn reset_removes_registry_and_immediately_drops_compiled_cache() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository.install_zip(&package_zip(SCRIPT)).unwrap();
    assert!(repository.compiled(&package()).is_ok());
    let workspace = ProxyWorkspace::default();

    repository
        .reset_application_bundle(application_document(workspace, Vec::new()))
        .await
        .unwrap();

    assert!(repository.list().unwrap().is_empty());
    assert!(repository.cache.lock().is_empty());
}

#[tokio::test]
async fn application_export_orders_versions_by_semver_not_text() {
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ));
    for version in ["10.0.0", "2.0.0", "1.0.0"] {
        let manifest = MANIFEST.replace("1.0.0", version);
        repository
            .install_zip(&package_zip_with_manifest(&manifest, SCRIPT))
            .unwrap();
    }

    let versions = repository
        .export_application_packages()
        .await
        .unwrap()
        .into_iter()
        .map(|package| package.package.version.as_str().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(versions, ["1.0.0", "2.0.0", "10.0.0"]);
}

fn application_document(
    workspace: ProxyWorkspace,
    packages: Vec<PortableApplicationProtocolPackage>,
) -> ApplicationConfigurationDocument {
    ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: workspace.id,
        workspaces: vec![workspace],
        settings: PortableSettings::from(&SettingsDraft::default()),
        certificate_materials: Vec::new(),
        protocol_packages: packages,
    }
}

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("portable-test").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn workspace_with_package() -> ProxyWorkspace {
    workspace_with_packages(&[package()])
}

fn workspace_requiring_upstream_encode() -> ProxyWorkspace {
    let mut workspace = workspace_with_package();
    let ListenerDataPlane::Socket(socket) = &mut workspace.listeners[0].data_plane else {
        unreachable!("test helper always creates a Socket listener")
    };
    let SocketPayloadProcessing::Scripted(scripted) = &mut socket.processing else {
        unreachable!("test helper always creates scripted processing")
    };
    scripted.upstream.encode_enabled = true;
    workspace
}

fn workspace_with_packages(packages: &[ProtocolPackageRef]) -> ProxyWorkspace {
    let listeners = packages
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, package)| ProxyListener {
            port: 18_080 + u16::try_from(index).unwrap(),
            data_plane: ListenerDataPlane::Socket(SocketRelaySettings::relay(
                SocketEndpoint {
                    host: "upstream.example.test".into(),
                    port: 9000,
                },
                SocketRelaySecurity::Transparent,
                1_000,
                SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                    package,
                    upstream: DirectionProcessingOptions {
                        decode_enabled: true,
                        encode_enabled: false,
                    },
                    downstream: DirectionProcessingOptions {
                        decode_enabled: true,
                        encode_enabled: false,
                    },
                }),
            )),
            ..ProxyListener::default()
        })
        .collect();
    ProxyWorkspace {
        listeners,
        ..ProxyWorkspace::default()
    }
}

fn package_zip(script: &str) -> Vec<u8> {
    package_zip_with_manifest(MANIFEST, script)
}

fn package_zip_with_manifest(manifest: &str, script: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for (path, bytes) in [
        ("manifest.toml", manifest.as_bytes()),
        ("document.toml", SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
    ] {
        writer.start_file(path, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[path = "portability_tests/atomic_replace.rs"]
mod atomic_replace;
#[path = "portability_tests/bindings.rs"]
mod bindings;
