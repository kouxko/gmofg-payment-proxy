use std::path::PathBuf;

use intercept_proxy_application::{AppResult, ProxyState};
use intercept_proxy_infrastructure::{
    InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
};
use intercept_proxy_product_api::InterceptProxyProfile;

use super::*;

#[derive(Debug)]
struct NoFileDialog;

impl NativeFileDialog for NoFileDialog {
    fn choose_open_file(&self, _purpose: &str) -> AppResult<Option<PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(
        &self,
        _purpose: &str,
        _suggested_file_name: &str,
    ) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct TestSecretProtector;

impl SecretProtector for TestSecretProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        self.protect(ciphertext)
    }
}

#[derive(Debug)]
struct RefusingSecretProtector;

impl SecretProtector for RefusingSecretProtector {
    fn protect(&self, _: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Err(InfrastructureError::KeychainProtect)
    }

    fn unprotect(&self, _: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Err(InfrastructureError::KeychainUnprotect)
    }
}

#[tokio::test]
async fn builds_and_invokes_application_without_tauri() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let host = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("build UI-neutral host");

    assert!(host.begin_shutdown(), "first caller owns graceful shutdown");
    assert!(
        !host.begin_shutdown(),
        "repeated callers must reuse the existing shutdown task"
    );
    assert!(!host.shutdown_completed());

    let application = host.application();
    let status = application
        .proxy_get_status()
        .await
        .expect("query proxy status");
    assert_eq!(status.state, ProxyState::Stopped);

    let settings = application.settings_get().await.expect("query settings");
    assert_eq!(settings.stored.max_sessions, 500);

    let draft = application
        .rule_new_draft()
        .await
        .expect("create rule draft");
    assert_eq!(draft.name, "新建规则");

    host.shutdown().await.expect("shutdown UI-neutral host");
    assert!(host.shutdown_completed());
}

#[tokio::test]
async fn keychain_refusal_does_not_prevent_host_or_bootstrap_startup() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let host = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(RefusingSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("host startup must not access the system secret store");
    let application = host.application();

    let bootstrap = application
        .app_bootstrap()
        .await
        .expect("metadata-only bootstrap remains available");
    assert!(bootstrap.certificate.can_initialize);

    let error = application
        .certificate_initialize_if_needed()
        .await
        .expect_err("explicit certificate initialization reports refusal");
    assert_eq!(error.view_model.code, "KEYCHAIN_PROTECT_FAILED");

    host.shutdown().await.expect("shutdown UI-neutral host");
}
