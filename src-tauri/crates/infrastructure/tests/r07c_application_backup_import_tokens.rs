use std::{
    io::{Cursor, Write},
    sync::{
        Arc, Barrier,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use intercept_proxy_application::{
    ApplicationBackupImportBaseline, ApplicationBackupImportCandidate,
    ApplicationBackupImportPreparePort, ApplicationBackupProtocolPackageBaseline,
    ApplicationBackupWorkspaceBaseline, PortableSettings, PreparedApplicationBackup, SettingsDraft,
};
use intercept_proxy_domain::{ProxyWorkspace, Revision};
use intercept_proxy_infrastructure::{
    ApplicationBackupArchiveLimits, ApplicationBackupImportClock, ApplicationBackupImportPreparer,
    ApplicationBackupImportTokenGenerator,
};
use serde_json::json;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

#[derive(Debug, Default)]
struct ManualClock(AtomicU64);

impl ManualClock {
    fn set(&self, seconds: u64) {
        self.0.store(seconds, Ordering::SeqCst);
    }
}

impl ApplicationBackupImportClock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Debug, Default)]
struct SequentialTokens(AtomicU64);

impl ApplicationBackupImportTokenGenerator for SequentialTokens {
    fn generate(&self) -> intercept_proxy_application::ApplicationBackupImportToken {
        let value = u128::from(self.0.fetch_add(1, Ordering::SeqCst)) + 1;
        intercept_proxy_application::ApplicationBackupImportToken::from_uuid(uuid::Uuid::from_u128(
            value,
        ))
    }
}

#[tokio::test]
async fn strict_archive_reconstruction_preserves_raw_payloads_and_reports_canonical_version() {
    let package_bytes = b"api = 1\nscript-secret";
    let certificate_bytes = b"certificate-secret";
    let zip = import_zip(package_bytes, certificate_bytes);

    let candidate = ApplicationBackupImportPreparer::new()
        .read(zip)
        .await
        .unwrap();

    assert_eq!(candidate.protocol_packages.len(), 1);
    assert_eq!(
        candidate.protocol_packages[0].files[0].path,
        "manifest.toml"
    );
    assert_eq!(
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &candidate.protocol_packages[0].files[0].contents_base64,
        )
        .unwrap(),
        package_bytes
    );
    assert_eq!(candidate.certificate_materials.len(), 1);
    assert_eq!(
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &candidate.certificate_materials[0].material_base64,
        )
        .unwrap(),
        certificate_bytes
    );
}

#[tokio::test]
async fn pending_capacity_rejects_only_the_candidate_beyond_the_bound() {
    let (preparer, _, _) = preparer(Duration::from_secs(30), 2);

    preparer.retain(prepared()).await.unwrap();
    preparer.retain(prepared()).await.unwrap();
    let error = preparer.retain(prepared()).await.unwrap_err();

    assert_eq!(error.view_model.code, "APPLICATION_BACKUP_IMPORT_CAPACITY");
}

#[tokio::test]
async fn pending_logical_byte_capacity_rejects_the_candidate_beyond_the_bound() {
    let clock = Arc::new(ManualClock::default());
    let tokens = Arc::new(SequentialTokens::default());
    let candidate = prepared();
    let max_pending_bytes = candidate.candidate.logical_bytes().unwrap();
    let preparer = ApplicationBackupImportPreparer::with_dependencies(
        ApplicationBackupArchiveLimits::default(),
        Duration::from_secs(30),
        2,
        max_pending_bytes,
        clock,
        tokens,
    )
    .unwrap();

    preparer.retain(candidate).await.unwrap();
    let error = preparer.retain(prepared()).await.unwrap_err();

    assert_eq!(error.view_model.code, "APPLICATION_BACKUP_IMPORT_CAPACITY");
}

#[tokio::test]
async fn consuming_a_token_releases_pending_capacity() {
    let (preparer, _, _) = preparer(Duration::from_secs(30), 1);
    let (token, _) = preparer.retain(prepared()).await.unwrap();

    preparer.take(token).unwrap();

    assert!(preparer.retain(prepared()).await.is_ok());
}

#[tokio::test]
async fn token_is_valid_immediately_before_ttl_and_expired_at_ttl() {
    let (valid_preparer, valid_clock, _) = preparer(Duration::from_secs(10), 1);
    let (valid_token, _) = valid_preparer.retain(prepared()).await.unwrap();
    valid_clock.set(9);
    assert!(valid_preparer.take(valid_token).is_ok());

    let (expired_preparer, expired_clock, _) = preparer(Duration::from_secs(10), 1);
    let (expired_token, _) = expired_preparer.retain(prepared()).await.unwrap();
    expired_clock.set(10);
    let error = expired_preparer.take(expired_token).unwrap_err();

    assert_eq!(
        error.view_model.code,
        "APPLICATION_BACKUP_IMPORT_TOKEN_EXPIRED"
    );
}

#[tokio::test]
async fn token_can_be_consumed_exactly_once() {
    let (preparer, _, _) = preparer(Duration::from_secs(30), 1);
    let (token, _) = preparer.retain(prepared()).await.unwrap();

    preparer.take(token).unwrap();
    let error = preparer.take(token).unwrap_err();

    assert_eq!(
        error.view_model.code,
        "APPLICATION_BACKUP_IMPORT_TOKEN_CONSUMED"
    );
}

#[tokio::test]
async fn concurrent_take_has_exactly_one_winner_without_timing_assumptions() {
    let (preparer, _, _) = preparer(Duration::from_secs(30), 1);
    let preparer = Arc::new(preparer);
    let (token, _) = preparer.retain(prepared()).await.unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let threads = (0..2)
        .map(|_| {
            let preparer = preparer.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                preparer.take(token)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let loser = results.into_iter().find_map(Result::err).unwrap();
    assert_eq!(
        loser.view_model.code,
        "APPLICATION_BACKUP_IMPORT_TOKEN_CONSUMED"
    );
}

#[tokio::test]
async fn discarded_token_cannot_be_consumed_and_releases_capacity() {
    let (preparer, _, _) = preparer(Duration::from_secs(30), 1);
    let (token, _) = preparer.retain(prepared()).await.unwrap();

    preparer.discard(token).await.unwrap();

    assert!(preparer.take(token).is_err());
    assert!(preparer.retain(prepared()).await.is_ok());
}

fn preparer(
    ttl: Duration,
    capacity: usize,
) -> (
    ApplicationBackupImportPreparer,
    Arc<ManualClock>,
    Arc<SequentialTokens>,
) {
    let clock = Arc::new(ManualClock::default());
    let tokens = Arc::new(SequentialTokens::default());
    let preparer = ApplicationBackupImportPreparer::with_dependencies(
        ApplicationBackupArchiveLimits::default(),
        ttl,
        capacity,
        u64::MAX,
        clock.clone(),
        tokens.clone(),
    )
    .unwrap();
    (preparer, clock, tokens)
}

fn prepared() -> PreparedApplicationBackup {
    let workspace = ProxyWorkspace::default();
    PreparedApplicationBackup {
        candidate: ApplicationBackupImportCandidate {
            selected_workspace_id: workspace.id,
            workspaces: vec![workspace.clone()],
            settings: PortableSettings::from(&SettingsDraft::default()),
            protocol_packages: Vec::new(),
            certificate_materials: Vec::new(),
        },
        baseline: ApplicationBackupImportBaseline {
            selected_workspace_id: workspace.id,
            workspaces: vec![ApplicationBackupWorkspaceBaseline {
                workspace_id: workspace.id,
                revision: workspace.revision,
            }],
            settings_revision: Revision::INITIAL,
            protocol_packages: Vec::<ApplicationBackupProtocolPackageBaseline>::new(),
            listener_certificate_generation: [0; 32],
        },
    }
}

fn import_zip(package_bytes: &[u8], certificate_bytes: &[u8]) -> Vec<u8> {
    let workspace = ProxyWorkspace {
        listeners: Vec::new(),
        ..ProxyWorkspace::default()
    };
    let application = serde_json::to_vec(&json!({
        "format_version": 1,
        "application": {
            "selected_workspace_id": workspace.id,
            "workspaces": [workspace],
            "settings": PortableSettings::from(&SettingsDraft::default()),
        },
        "protocol_packages": [{
            "package": { "id": "sample", "version": "1.0.0" },
            "enabled": true,
            "files": ["protocol-packages/sample/1.0.0/manifest.toml"],
        }],
        "portable_materials": [{
            "reference_id": uuid::Uuid::from_u128(42),
            "label": "server identity",
            "kind": "reverse_server_identity",
            "path": "portable-materials/server-identity.p12",
            "password": "password-secret",
        }],
    }))
    .unwrap();
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (path, bytes) in [
            ("application.json", application.as_slice()),
            (
                "protocol-packages/sample/1.0.0/manifest.toml",
                package_bytes,
            ),
            ("portable-materials/server-identity.p12", certificate_bytes),
        ] {
            writer.start_file(path, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    output.into_inner()
}
