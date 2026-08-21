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

fn test_listener_runtime(store: Arc<SqliteStore>) -> ListenerRuntimeAdapter {
    let protocol_packages = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    test_listener_runtime_with_packages(store, protocol_packages)
}

fn test_listener_runtime_with_packages(
    store: Arc<SqliteStore>,
    protocol_packages: Arc<ProtocolPackageRepositoryAdapter>,
) -> ListenerRuntimeAdapter {
    let protected_secrets = Arc::new(ProtectedSecretAdapter::new(
        Arc::clone(&store),
        Arc::new(ListenerRuntimeTestProtector),
    ));
    ListenerRuntimeAdapter::new(store, protected_secrets, protocol_packages)
}

include!("tests/certificate_policy.rs");
include!("tests/forward_proxy.rs");
include!("tests/fixed_server.rs");
include!("tests/socket_runtime.rs");
include!("tests/validation.rs");

#[path = "tests/external_package_runtime.rs"]
mod external_package_runtime_tests;
#[path = "tests/http_protocol_pipeline.rs"]
mod http_protocol_pipeline_tests;
#[path = "tests/local_responder_runtime.rs"]
mod local_responder_runtime_tests;
#[path = "tests/scripted_relay_runtime.rs"]
mod scripted_relay_runtime_tests;
#[path = "tests/scripted_snapshot.rs"]
mod scripted_snapshot_tests;
