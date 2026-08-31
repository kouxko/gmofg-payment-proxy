use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
};

use intercept_proxy_application::{
    EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentCommitTarget, HttpBodyProcessing, builtin_iso8583_package_ref,
};
use intercept_proxy_domain::{ListenerDataPlane, ProxyWorkspace};
use zip::{ZipWriter, write::SimpleFileOptions};

use super::runtime_fixture_with_builtin;

#[tokio::test(flavor = "current_thread")]
async fn phase13_seed_projects_the_enabled_builtin_before_sidecar_start() {
    let fixture = runtime_fixture_with_builtin(Some(Arc::from(template_zip()))).await;
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

fn template_zip() -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../templates/socket-protocol/iso8583-standard");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for relative in ["manifest.json", "protocol.js", "display.js"] {
        writer
            .start_file(relative, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&fs::read(root.join(relative)).unwrap())
            .unwrap();
    }
    writer.finish().unwrap().into_inner()
}
