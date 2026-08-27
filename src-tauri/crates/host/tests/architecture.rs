//! 依赖方向的“防回退”测试。
//!
//! 这些测试扫描 Cargo 元数据和源码，防止 Tauri 或已删除的旧产品契约重新渗入通用核心，
//! 同时固定动态 Workspace 编解码器的依赖方向。它们不证明运行时网络行为。

use std::path::Path;

#[path = "architecture/support.rs"]
mod support;

use support::{
    assert_no_tauri_dependency, crates_dir, is_test_source, remove_cfg_test_items,
    resolved_dependencies, rust_sources,
};

const CORE_CRATES: [(&str, &str); 9] = [
    ("domain", "intercept-proxy-domain"),
    ("application", "intercept-proxy-application"),
    ("exchange", "intercept-proxy-exchange"),
    ("android-engine", "intercept-proxy-android-engine"),
    ("proxy", "intercept-proxy-runtime"),
    ("product-api", "intercept-proxy-product-api"),
    ("infrastructure", "intercept-proxy-infrastructure"),
    ("protocol-scripting", "intercept-proxy-protocol-scripting"),
    ("host", "intercept-proxy-host"),
];

#[test]
fn reusable_rust_crates_do_not_depend_on_tauri() {
    let crates_dir = crates_dir();

    for (directory, package_name) in CORE_CRATES {
        let manifest_path = crates_dir.join(directory).join("Cargo.toml");
        let manifest = std::fs::read_to_string(&manifest_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", manifest_path.display()));
        assert_no_tauri_dependency(&manifest_path, &manifest);

        let packages = resolved_dependencies(package_name);
        assert!(
            packages.iter().all(|package| !package.starts_with("tauri")),
            "{package_name} must stay UI-neutral, resolved dependencies include: {packages:?}"
        );
    }
}

#[test]
fn runtime_crate_does_not_depend_on_application_layer() {
    let packages = resolved_dependencies("intercept-proxy-runtime");
    assert!(
        !packages
            .iter()
            .any(|package| package == "intercept-proxy-application"),
        "proxy runtime must not depend upward on application: {packages:?}"
    );
}

#[test]
fn generic_core_does_not_depend_on_removed_legacy_product_fixture() {
    for crate_name in [
        "intercept-proxy-domain",
        "intercept-proxy-application",
        "intercept-proxy-exchange",
        "intercept-proxy-android-engine",
        "intercept-proxy-runtime",
        "intercept-proxy-product-api",
        "intercept-proxy-infrastructure",
        "intercept-proxy-protocol-scripting",
        "intercept-proxy-host",
    ] {
        let packages = resolved_dependencies(crate_name);
        assert!(
            !packages
                .iter()
                .any(|package| package == "intercept-proxy-legacy-test-fixture"),
            "{crate_name} must not depend on the removed legacy product fixture: {packages:?}"
        );
    }
}

#[test]
fn dynamic_workspace_body_codecs_stay_in_infrastructure() {
    for crate_name in [
        "intercept-proxy-domain",
        "intercept-proxy-application",
        "intercept-proxy-runtime",
    ] {
        let packages = resolved_dependencies(crate_name);
        assert!(
            !packages.iter().any(|package| package == "encoding_rs"),
            "{crate_name} must remain independent of concrete text encodings: {packages:?}"
        );
    }
    let infrastructure = resolved_dependencies("intercept-proxy-infrastructure");
    assert!(
        infrastructure
            .iter()
            .any(|package| package == "encoding_rs"),
        "infrastructure must implement the Workspace-selectable Shift-JIS codec: {infrastructure:?}"
    );
}

#[test]
fn infrastructure_bundle_is_consumed_only_by_the_host_composition_root() {
    let host_source = crates_dir().join("host/src");
    let allowed = host_source.join("lib.rs");

    for source in rust_sources(&host_source) {
        if source == allowed || is_test_source(&source) {
            continue;
        }
        let text = std::fs::read_to_string(&source)
            .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
        let production = remove_cfg_test_items(&text);
        assert!(
            !production.contains("InfrastructureServiceBundle"),
            "{} must accept only the capabilities it uses, not the whole infrastructure bundle",
            source.display()
        );
    }

    let root = std::fs::read_to_string(&allowed)
        .unwrap_or_else(|error| panic!("read {}: {error}", allowed.display()));
    let production = remove_cfg_test_items(&root);
    assert_eq!(
        production.matches("&InfrastructureServiceBundle").count(),
        0,
        "composition helpers must accept explicit capabilities instead of borrowing the whole bundle"
    );
    assert!(
        !production.contains("Arc<InfrastructureServiceBundle")
            && !production.contains("InfrastructureServiceBundle>"),
        "the host must own the infrastructure bundle directly only in its composition root"
    );
}

#[test]
fn infrastructure_root_does_not_export_implementation_only_adapters() {
    let infrastructure_root = crates_dir().join("infrastructure/src/lib.rs");
    let source = std::fs::read_to_string(&infrastructure_root)
        .unwrap_or_else(|error| panic!("read {}: {error}", infrastructure_root.display()));
    let adapter_exports = source
        .split_once("pub use adapters::{")
        .and_then(|(_, suffix)| suffix.split_once("};"))
        .map(|(exports, _)| exports)
        .expect("infrastructure adapter root export block");
    let implementation_only = [
        "AcceptedExternalPackageConnection",
        "BoundSocketDocument",
        "ExternalPackageConnectionId",
        "ExternalPackageFatalProtocolError",
        "ProtocolDocumentRuleConnection",
        "ProtocolDocumentRuleConnectionFactory",
        "ProtocolPackageStorageError",
        "ProtocolPackageStorageErrorCode",
        "ProtocolPackageSummary",
        "RuntimeRuleRepository",
        "accept_packages_websocket",
        "external_package_registration_fingerprint",
        "CaptureRepositoryAdapter",
        "ExternalPackageConnectionConfig",
        "ExternalPackageListenerRuntime",
        "ExternalPackageRegistryAdapter",
        "ExternalPackageServerConfig",
        "HeaderBodyCodecResolver",
        "ProtocolPackageRepositoryAdapter",
        "RuleRepositoryAdapter",
        "RuntimePipelineAdapter",
        "RuntimePipelineProductHooks",
        "SettingsRepositoryAdapter",
        "WorkspaceBodyCodecResolver",
    ];

    for symbol in implementation_only {
        assert!(
            !adapter_exports.contains(symbol),
            "infrastructure root must not re-export implementation-only symbol {symbol}"
        );
    }
}

#[test]
fn infrastructure_adapters_module_is_private_and_not_used_cross_crate() {
    let infrastructure_root = crates_dir().join("infrastructure/src/lib.rs");
    let source = std::fs::read_to_string(&infrastructure_root)
        .unwrap_or_else(|error| panic!("read {}: {error}", infrastructure_root.display()));
    assert!(source.contains("mod adapters;"));
    assert!(!source.contains("pub mod adapters;"));

    for (directory, _) in CORE_CRATES {
        if directory == "infrastructure" {
            continue;
        }
        for source in rust_sources(&crates_dir().join(directory)) {
            if source == crates_dir().join("host/tests/architecture.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&source)
                .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
            assert!(
                !text.contains("intercept_proxy_infrastructure::adapters::")
                    && !text.contains("adapters::FileSelection"),
                "{} must use intentional infrastructure root contracts, not its adapters module",
                source.display()
            );
        }
    }
    for source in rust_sources(&crates_dir().parent().expect("workspace root").join("src")) {
        let text = std::fs::read_to_string(&source)
            .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
        assert!(
            !text.contains("intercept_proxy_infrastructure::adapters::")
                && !text.contains("adapters::FileSelection"),
            "{} must use intentional infrastructure root contracts, not its adapters module",
            source.display()
        );
    }
}

#[test]
fn infrastructure_bundle_fields_are_private_and_host_avoids_adapter_concretes() {
    let bundle =
        std::fs::read_to_string(crates_dir().join("infrastructure/src/adapters/bundle.rs"))
            .expect("read infrastructure bundle");
    let fields = bundle
        .split_once("pub struct InfrastructureServiceBundle {")
        .and_then(|(_, suffix)| suffix.split_once("}\n\nimpl InfrastructureServiceBundle"))
        .map(|(fields, _)| fields)
        .expect("bundle fields");
    assert!(
        !fields
            .lines()
            .any(|line| line.trim_start().starts_with("pub ")),
        "infrastructure bundle must expose intent methods rather than public adapter fields"
    );

    let host = std::fs::read_to_string(crates_dir().join("host/src/lib.rs"))
        .expect("read host composition root");
    for concrete in [
        "AndroidAdbAdapter",
        "CaptureRepositoryAdapter",
        "ExternalPackageRegistryAdapter",
        "HeaderBodyCodecResolver",
        "ProtocolPackageRepositoryAdapter",
        "RuleRepositoryAdapter",
        "RuntimePipelineAdapter",
        "RuntimePipelineProductHooks",
        "SettingsRepositoryAdapter",
        "WorkspaceBodyCodecResolver",
    ] {
        assert!(
            !host.contains(concrete),
            "Host must request infrastructure intent instead of importing {concrete}"
        );
    }
}

#[test]
fn removed_legacy_product_fixture_is_not_a_workspace_member() {
    let manifest = std::fs::read_to_string(crates_dir().parent().unwrap().join("Cargo.toml"))
        .expect("read workspace manifest");
    assert!(!manifest.contains("product-payment"));
    assert!(!crates_dir().join("product-payment").exists());
}

#[test]
fn generic_production_sources_do_not_contain_removed_product_contracts() {
    let forbidden = [
        "GMO-FG",
        "Payment App",
        "D48",
        "ChannelKind::Transaction",
        "ChannelKind::Dll",
        "enum ChannelKind",
        "transaction_port",
        "dll_port",
        "upstream_transaction_url",
        "upstream_dll_url",
        "16_627",
        "16_127",
        "gmofg-payment-proxy.sqlite3",
        "com.gmofg.payment-proxy",
        "gmofg-payment-proxy/keychain",
    ];

    for directory in [
        "domain",
        "application",
        "android-engine",
        "proxy",
        "infrastructure",
        "host",
    ] {
        let source_dir = crates_dir().join(directory).join("src");
        for source in rust_sources(&source_dir) {
            // `tests/` 与 `*_tests.rs` 只会由父模块的 `#[cfg(test)]` 声明引入。
            // 外部模块文件本身看不到父文件上的属性，因此不能仅靠文本剥离识别它们。
            if is_test_source(&source) {
                continue;
            }
            let text = std::fs::read_to_string(&source)
                .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
            let production = remove_cfg_test_items(&text);
            for term in forbidden {
                assert!(
                    !production.contains(term),
                    "{} generic production source contains product contract {term:?}",
                    source.display()
                );
            }
        }
    }
}

#[test]
fn product_contract_scan_keeps_production_after_test_only_items() {
    let source = r####"
const BEFORE: &str = "generic";
#[cfg(test)]
const TEST_ONLY: &str = "GMO-FG";
const AFTER_CONST: &str = "production-after-const";
#[cfg(test)]
fn helper() {
    let raw = r###"GMO-FG { test }"###;
    assert!(!raw.is_empty());
}
const AFTER_FUNCTION: &str = "production-after-function";
#[cfg(test)]
mod tests {
    const FIXTURE: &str = "GMO-FG";
}
const AFTER_MODULE: &str = "production-after-module";
"####;

    let production = remove_cfg_test_items(source);

    assert!(!production.contains("GMO-FG"));
    assert!(production.contains("production-after-const"));
    assert!(production.contains("production-after-function"));
    assert!(production.contains("production-after-module"));
}

#[test]
fn product_contract_scan_recognizes_split_test_modules() {
    assert!(is_test_source(Path::new("src/facade/listeners_tests.rs")));
    assert!(is_test_source(Path::new(
        "src/adapters/android_adb/tests.rs"
    )));
    assert!(is_test_source(Path::new(
        "src/adapters/android_adb/tests/reverse.rs"
    )));
    assert!(!is_test_source(Path::new("src/facade/listeners.rs")));
}

#[test]
fn product_contract_scan_removes_cfg_test_struct_fields() {
    let source = r"
struct ProductionState {
    value: u64,
    #[cfg(test)]
    test_gate: Option<String>,
}

fn production_value() -> u64 { 7 }
";

    let production = remove_cfg_test_items(source);
    assert!(production.contains("value: u64"));
    assert!(production.contains("production_value"));
    assert!(!production.contains("test_gate"));
}
