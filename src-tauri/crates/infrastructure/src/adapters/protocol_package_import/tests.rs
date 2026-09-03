use std::{
    borrow::Cow,
    io::{Cursor, Write},
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_application::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, AppResult, ApplicationConfigurationDocument,
    ExternalPackageApplicationPort, PortableSettings, ProtocolPackageImportPort, SettingsDraft,
};
use intercept_proxy_domain::{ProtocolDirection, ProxyWorkspace};
use intercept_proxy_package_contract::FrameResult;
use tempfile::TempDir;
use wasm_encoder::{Component, CustomSection};
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;
use crate::{
    SqliteStore,
    adapters::{
        FileSelection, ProtocolPackageRuntime, listener_runtime::ExternalSocketPackageProvider,
    },
};

const MANIFEST: &str = include_str!(
    "../../../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);

#[derive(Debug, Default)]
struct QueueDialog(Mutex<Vec<PathBuf>>);
impl NativeFileDialog for QueueDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        Ok(Some(self.0.lock().unwrap().remove(0)))
    }
    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
        unreachable!()
    }
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut output);
    for (path, bytes) in entries {
        writer
            .start_file(*path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
    output.into_inner()
}

fn component(manifest: &[u8]) -> Vec<u8> {
    let mut component = Component::new();
    component.section(&CustomSection {
        name: Cow::Borrowed(intercept_proxy_package_runtime::PACKAGE_MANIFEST_SECTION),
        data: Cow::Borrowed(manifest),
    });
    component.finish()
}

fn built_in_socket_component() -> Vec<u8> {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(build_socket_component).clone()
}

fn build_socket_component() -> Vec<u8> {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let source = repository.join("templates/socket-protocol/iso8583-standard");
    let target = TempDir::new().unwrap();
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([
            "build",
            "--locked",
            "--manifest-path",
            source.join("Cargo.toml").to_str().unwrap(),
            "--target",
            "wasm32-wasip2",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let component = std::fs::read(
        target
            .path()
            .join("wasm32-wasip2/debug/intercept_proxy_iso8583_ascii_standard_component.wasm"),
    )
    .unwrap();
    let manifest = std::fs::read(source.join("manifest.json")).unwrap();
    intercept_proxy_package_runtime::embed_package_manifest(&component, &manifest).unwrap()
}

fn application_document(
    packages: Vec<intercept_proxy_application::PortableApplicationProtocolPackage>,
) -> ApplicationConfigurationDocument {
    let workspace = ProxyWorkspace::default();
    ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: workspace.id,
        workspaces: vec![workspace],
        settings: PortableSettings::from(&SettingsDraft::default()),
        certificate_materials: Vec::new(),
        protocol_packages: packages,
    }
}

fn importer(
    bytes: &[u8],
) -> (
    TempDir,
    ProtocolPackageImportAdapter,
    Arc<ExternalPackageRegistryAdapter>,
    Arc<SqliteStore>,
) {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("package.wasm");
    std::fs::write(&path, bytes).unwrap();
    let dialog = Arc::new(QueueDialog(Mutex::new(vec![path])));
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::clone(&store)));
    (
        temp,
        ProtocolPackageImportAdapter::new(Arc::clone(&registry), dialog),
        registry,
        store,
    )
}

#[tokio::test]
async fn component_manifest_previews_without_instantiating_guest_exports() {
    let bytes = component(MANIFEST.as_bytes());
    let (_temp, importer, _registry, _store) = importer(&bytes);
    let preview = importer.prepare_component().await.unwrap().unwrap();
    assert_eq!(
        preview.disposition,
        ProtocolPackageImportDispositionViewModel::New
    );
    assert!(preview.token.is_some());
    assert_eq!(preview.host_api, 1);
    assert!(!preview.capabilities.upstream.frame);
    assert!(!preview.capabilities.downstream.frame);
    assert!(preview.upstream_schema.is_none());
    assert!(preview.downstream_schema.is_some());
}

#[tokio::test]
async fn commit_failure_leaves_no_persisted_component_or_runtime() {
    let bytes = component(MANIFEST.as_bytes());
    let (_temp, importer, registry, _store) = importer(&bytes);
    let preview = importer.prepare_component().await.unwrap().unwrap();
    let package = preview.package.clone();
    let error = importer
        .commit_component(preview.token.unwrap())
        .await
        .unwrap_err();
    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_INVALID");
    assert!(registry.get(&package).await.unwrap().is_none());
    assert!(
        ExternalSocketPackageProvider::resolve(registry.as_ref(), &package)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn valid_component_import_persists_activates_disables_and_restarts_locally() {
    let bytes = built_in_socket_component();
    let (_temp, importer, registry, _store) = importer(&bytes);
    let preview = importer.prepare_component().await.unwrap().unwrap();
    let package = preview.package.clone();
    let committed = importer
        .commit_component(preview.token.unwrap())
        .await
        .unwrap();
    assert!(committed.version.enabled);
    assert!(matches!(
        committed.version.source,
        intercept_proxy_application::ProtocolPackageSourceViewModel::Managed { online: true }
    ));
    assert_eq!(
        registry.local_archive(&package).await.unwrap().unwrap(),
        bytes
    );

    let binding = ExternalSocketPackageProvider::resolve(registry.as_ref(), &package)
        .await
        .unwrap()
        .unwrap();
    let active_listener_runtime = binding.runtime();
    assert_eq!(binding.max_frame_bytes(), usize::MAX);
    let request = vec![0_u8, 4, b'0', b'2', b'0', b'0'];
    assert_eq!(
        binding
            .runtime()
            .frame(ProtocolDirection::Upstream, request.clone())
            .await
            .unwrap(),
        FrameResult::complete(request.len()).unwrap()
    );

    registry.set_enabled(&package, false).await.unwrap();
    assert!(
        active_listener_runtime
            .frame(ProtocolDirection::Upstream, request.clone())
            .await
            .is_err(),
        "a binding captured before disable must observe the stopped runtime"
    );
    let disabled = ExternalSocketPackageProvider::resolve(registry.as_ref(), &package)
        .await
        .unwrap_err();
    assert_eq!(disabled.view_model.code, "EXTERNAL_PACKAGE_DISABLED");

    registry.set_enabled(&package, true).await.unwrap();
    assert!(
        active_listener_runtime
            .frame(ProtocolDirection::Upstream, request.clone())
            .await
            .is_ok(),
        "enable must repopulate the stable runtime handle held by an active listener"
    );
    registry.restart(&package).await.unwrap();
    assert!(
        active_listener_runtime
            .frame(ProtocolDirection::Upstream, request.clone())
            .await
            .is_ok(),
        "restart must replace the instance behind the stable listener handle"
    );
    let restarted = ExternalSocketPackageProvider::resolve(registry.as_ref(), &package)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        restarted
            .runtime()
            .frame(ProtocolDirection::Downstream, request)
            .await,
        Ok(FrameResult::Complete { consumed_bytes }) if consumed_bytes.get() == 6
    ));

    registry.delete(&package).await.unwrap();
    assert!(
        active_listener_runtime
            .frame(
                ProtocolDirection::Upstream,
                vec![0_u8, 4, b'0', b'2', b'0', b'0']
            )
            .await
            .is_err(),
        "a binding captured before delete must remain permanently offline"
    );
    assert!(registry.get(&package).await.unwrap().is_none());
    assert!(
        ExternalSocketPackageProvider::resolve(registry.as_ref(), &package)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn enabled_component_is_reinstantiated_from_sqlite_on_cold_start() {
    let bytes = built_in_socket_component();
    let (_temp, importer, _registry, store) = importer(&bytes);
    let preview = importer.prepare_component().await.unwrap().unwrap();
    let package = preview.package.clone();
    importer
        .commit_component(preview.token.unwrap())
        .await
        .unwrap();

    let restored = ExternalPackageRegistryAdapter::new(store);
    let offline = ExternalSocketPackageProvider::resolve(&restored, &package)
        .await
        .unwrap_err();
    assert_eq!(offline.view_model.code, "PROTOCOL_PACKAGE_RUNTIME_OFFLINE");
    for (stored_package, component) in restored.enabled_local_archives().await.unwrap() {
        restored
            .activate_local_component(&stored_package, &component)
            .await
            .unwrap();
    }

    let binding = ExternalSocketPackageProvider::resolve(&restored, &package)
        .await
        .unwrap()
        .unwrap();
    let request = vec![0_u8, 4, b'0', b'2', b'0', b'0'];
    assert_eq!(
        binding
            .runtime()
            .frame(ProtocolDirection::Upstream, request.clone())
            .await
            .unwrap(),
        FrameResult::complete(request.len()).unwrap()
    );
}

#[tokio::test]
async fn corrupted_disabled_component_cannot_enable_or_change_the_persisted_flag() {
    let bytes = built_in_socket_component();
    let (_temp, importer, registry, store) = importer(&bytes);
    let preview = importer.prepare_component().await.unwrap().unwrap();
    let package = preview.package.clone();
    importer
        .commit_component(preview.token.unwrap())
        .await
        .unwrap();
    registry.set_enabled(&package, false).await.unwrap();
    store.replace_local_archive_for_test(&package, b"not a Component");

    let error = registry.set_enabled(&package, true).await.unwrap_err();
    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_INVALID");
    let stored = registry.get(&package).await.unwrap().unwrap();
    assert!(!stored.enabled);
    assert!(matches!(
        stored.source,
        intercept_proxy_application::ProtocolPackageSourceViewModel::Managed { online: false }
    ));
}

#[tokio::test]
async fn application_backup_round_trips_raw_component_and_preserves_stable_runtime_identity() {
    let bytes = built_in_socket_component();
    let (_temp, importer, registry, store) = importer(&bytes);
    let preview = importer.prepare_component().await.unwrap().unwrap();
    let package = preview.package.clone();
    importer
        .commit_component(preview.token.unwrap())
        .await
        .unwrap();
    let old_runtime = registry.active_local_runtime(&package).unwrap();

    let exported = registry.export_application_packages().await.unwrap();
    assert_eq!(exported.len(), 1);
    assert!(exported[0].enabled);
    assert_eq!(exported[0].files[0].path, "component.wasm");
    assert_eq!(
        STANDARD
            .decode(&exported[0].files[0].contents_base64)
            .unwrap(),
        bytes
    );
    assert_eq!(
        registry.application_backup_baseline().await.unwrap().len(),
        1
    );
    assert_eq!(
        registry
            .preflight_application_packages(&exported)
            .await
            .unwrap()[0]
            .package,
        package
    );

    registry
        .replace_application_bundle(exported.clone(), application_document(exported))
        .await
        .unwrap();
    let replaced = registry.active_local_runtime(&package).unwrap();
    assert!(Arc::ptr_eq(&old_runtime, &replaced));
    let request = vec![0_u8, 4, b'0', b'2', b'0', b'0'];
    assert_eq!(
        old_runtime
            .frame(ProtocolDirection::Upstream, request.clone())
            .await
            .unwrap(),
        FrameResult::complete(request.len()).unwrap()
    );
    assert_eq!(
        store.list_external_packages().unwrap()[0].local_archive,
        Some(bytes)
    );

    registry
        .reset_application_bundle(application_document(Vec::new()))
        .await
        .unwrap();
    assert!(store.list_external_packages().unwrap().is_empty());
    assert!(
        old_runtime
            .frame(ProtocolDirection::Upstream, request)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn legacy_zip_packages_are_rejected_before_prepare() {
    for entries in [
        vec![
            ("manifest.json", b"api=1".as_slice()),
            ("protocol.js", b"fn x(){}".as_slice()),
        ],
        vec![
            ("package/manifest.json", MANIFEST.as_bytes()),
            ("package/protocol.js", b"export {}"),
            ("package/display.js", b"export {}"),
        ],
    ] {
        let bytes = zip(&entries);
        let (_temp, importer, _registry, _store) = importer(&bytes);
        let error = importer.prepare_component().await.unwrap_err();
        assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_INVALID");
        assert!(!error.view_model.field_errors.contains_key("runtime"));
    }
}
