use std::{path::PathBuf, process::Command, sync::Arc};

use intercept_proxy_application::{
    EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentCommitTarget, HttpBodyProcessing, builtin_iso8583_package_ref,
};
use intercept_proxy_domain::{ListenerDataPlane, ProxyWorkspace};

use super::runtime_fixture_with_builtin;

#[tokio::test(flavor = "current_thread")]
async fn seeded_builtin_is_projected_before_runtime_instantiation() {
    let fixture = runtime_fixture_with_builtin(Some(Arc::from(template_component()))).await;
    let mut candidate = ProxyWorkspace::default();
    let ListenerDataPlane::Http(settings) = &mut candidate.listeners[0].data_plane else {
        panic!("default Listener is HTTP")
    };
    settings.body_processing = HttpBodyProcessing::Protocol {
        package: builtin_iso8583_package_ref(),
    };

    let baseline = fixture
        .runtime
        .capture(EnvironmentApplyBaselineCaptureRequest {
            target: EnvironmentCommitTarget::New {
                workspace_id: candidate.id.as_uuid(),
                display_name: candidate.name.clone(),
            },
            persisted_workspace: None,
            candidate_workspace: candidate,
            schema_version: 1,
            validation_engine_version:
                intercept_proxy_application::ENVIRONMENT_VALIDATION_ENGINE_VERSION,
        })
        .await
        .expect("builtin exact package must be observable during baseline capture");

    assert_eq!(baseline.exact_packages().len(), 1);
    assert_eq!(
        baseline.exact_packages()[0].package_ref(),
        &builtin_iso8583_package_ref()
    );
    assert!(baseline.exact_packages()[0].enabled());
    assert!(!baseline.exact_packages()[0].online());
}

fn template_component() -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../templates/socket-protocol/iso8583-standard");
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "--manifest-path",
            root.join("Cargo.toml").to_str().unwrap(),
            "--target",
            "wasm32-wasip2",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let component =
        std::fs::read(root.join(
            "target/wasm32-wasip2/debug/intercept_proxy_iso8583_ascii_standard_component.wasm",
        ))
        .unwrap();
    let manifest = std::fs::read(root.join("manifest.json")).unwrap();
    intercept_proxy_package_runtime::embed_package_manifest(&component, &manifest).unwrap()
}
