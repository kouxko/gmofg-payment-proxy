#[derive(Debug)]
struct ListenerRuntimeTestProtector;

impl crate::SecretProtector for ListenerRuntimeTestProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, crate::InfrastructureError> {
        Ok(plaintext.to_vec())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, crate::InfrastructureError> {
        Ok(ciphertext.to_vec())
    }
}

pub(super) fn test_listener_runtime(store: Arc<SqliteStore>) -> ListenerRuntimeAdapter {
    let protected_secrets = Arc::new(ProtectedSecretAdapter::new(
        Arc::clone(&store),
        Arc::new(ListenerRuntimeTestProtector),
    ));
    ListenerRuntimeAdapter::new(store, protected_secrets)
}

include!("tests/certificate_policy.rs");
include!("tests/body_codec_lifecycle.rs");
include!("tests/body_codec_epoch_cleanup.rs");
include!("tests/body_codec_cancellation.rs");
include!("tests/forward_proxy.rs");
include!("tests/fixed_server.rs");
include!("tests/socket_runtime.rs");
include!("tests/validation.rs");
include!("tests/environment_apply_gate_revision16.rs");

#[path = "tests/external_package_runtime.rs"]
mod external_package_runtime_tests;
#[path = "tests/phase10_http_pipeline.rs"]
mod phase10_http_pipeline_tests;
#[path = "tests/runtime_epoch_aba.rs"]
mod runtime_epoch_aba_tests;
