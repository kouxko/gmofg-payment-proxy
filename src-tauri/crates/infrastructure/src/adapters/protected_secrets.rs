//! 系统密钥保护的代理认证秘密适配器。
//!
//! `SQLite` 只保存受 Keychain/DPAPI 保护的完整 `Proxy-Authorization` 值。运行时启动
//! Listener 时短暂解密到可自动清零的内存，并使用常量时间比较；用户名、密码和 Header
//! 都不会进入 Workspace、日志或 UI 返回值。

use std::{fmt, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use intercept_proxy_application::{AppError, AppResult, ProtectedSecretPort, SecretReference};
use intercept_proxy_runtime::ForwardProxyAuthenticator;
use ring::hmac;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::{ProtectedSecretRecord, SecretProtector, SqliteStore};

use super::common::infra;

const PROVIDER: &str = "system";

pub struct ProtectedSecretAdapter {
    store: Arc<SqliteStore>,
    protector: Arc<dyn SecretProtector>,
}

impl fmt::Debug for ProtectedSecretAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtectedSecretAdapter")
            .field("store", &self.store)
            .field("protector", &"<system secret protector>")
            .finish()
    }
}

impl ProtectedSecretAdapter {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>, protector: Arc<dyn SecretProtector>) -> Self {
        Self { store, protector }
    }

    pub fn resolve_basic_authenticator(
        &self,
        reference: &SecretReference,
    ) -> AppResult<Arc<dyn ForwardProxyAuthenticator>> {
        if reference.provider != PROVIDER || reference.key.trim().is_empty() {
            return Err(AppError::new(
                "SECRET_REFERENCE_INVALID",
                "代理认证安全引用无效或不属于当前安装实例。",
            ));
        }
        let record = infra(
            self.store
                .load_protected_secret(&reference.provider, &reference.key),
        )?
        .ok_or_else(|| {
            AppError::new(
                "SECRET_NOT_FOUND",
                "代理认证安全引用不存在，请重新输入用户名和密码。",
            )
        })?;
        let plaintext = Zeroizing::new(infra(self.protector.unprotect(&record.protected_blob))?);
        let comparison_key = hmac::Key::new(
            hmac::HMAC_SHA256,
            b"intercept-proxy/basic-auth-constant-time/v1",
        );
        let expected_tag =
            Zeroizing::new(hmac::sign(&comparison_key, &plaintext).as_ref().to_vec());
        Ok(Arc::new(ConstantTimeBasicAuthenticator {
            comparison_key,
            expected_tag,
        }))
    }
}

#[async_trait]
impl ProtectedSecretPort for ProtectedSecretAdapter {
    async fn store_basic_auth(
        &self,
        username: String,
        mut password: String,
    ) -> AppResult<SecretReference> {
        let mut joined = Zeroizing::new(Vec::with_capacity(username.len() + password.len() + 1));
        joined.extend_from_slice(username.as_bytes());
        joined.push(b':');
        joined.extend_from_slice(password.as_bytes());
        password.zeroize();

        let encoded = STANDARD.encode(&*joined);
        let mut authorization = Zeroizing::new(Vec::with_capacity(6 + encoded.len()));
        authorization.extend_from_slice(b"Basic ");
        authorization.extend_from_slice(encoded.as_bytes());
        let protected_blob = infra(self.protector.protect(&authorization))?;

        let key = Uuid::new_v4().to_string();
        infra(self.store.save_protected_secret(&ProtectedSecretRecord {
            provider: PROVIDER.into(),
            key: key.clone(),
            protected_blob,
            updated_at: Utc::now(),
        }))?;
        Ok(SecretReference {
            provider: PROVIDER.into(),
            key,
        })
    }
}

struct ConstantTimeBasicAuthenticator {
    comparison_key: hmac::Key,
    expected_tag: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for ConstantTimeBasicAuthenticator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConstantTimeBasicAuthenticator")
            .field("expected_tag", &"<redacted>")
            .finish()
    }
}

impl ForwardProxyAuthenticator for ConstantTimeBasicAuthenticator {
    fn authorize(&self, _peer: SocketAddr, presented: Option<&http::HeaderValue>) -> bool {
        presented.is_some_and(|value| {
            hmac::verify(&self.comparison_key, value.as_bytes(), &self.expected_tag).is_ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InfrastructureError;

    #[derive(Debug)]
    struct TestProtector;

    impl SecretProtector for TestProtector {
        fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            Ok(plaintext.iter().map(|byte| byte ^ 0xA5).collect())
        }

        fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            Ok(ciphertext.iter().map(|byte| byte ^ 0xA5).collect())
        }
    }

    #[tokio::test]
    async fn basic_secret_round_trip_only_returns_reference_and_authenticates_exact_header() {
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let adapter = ProtectedSecretAdapter::new(store.clone(), Arc::new(TestProtector));
        let reference = adapter
            .store_basic_auth("operator".into(), "secret".into())
            .await
            .unwrap();

        assert_eq!(reference.provider, PROVIDER);
        assert!(!reference.key.contains("operator"));
        let persisted = store
            .load_protected_secret(&reference.provider, &reference.key)
            .unwrap()
            .unwrap();
        assert_ne!(persisted.protected_blob, b"Basic b3BlcmF0b3I6c2VjcmV0");

        let authenticator = adapter.resolve_basic_authenticator(&reference).unwrap();
        let peer = "127.0.0.1:12345".parse().unwrap();
        assert!(authenticator.authorize(
            peer,
            Some(&http::HeaderValue::from_static(
                "Basic b3BlcmF0b3I6c2VjcmV0"
            ))
        ));
        assert!(
            !authenticator.authorize(peer, Some(&http::HeaderValue::from_static("Basic invalid")))
        );
        assert!(!authenticator.authorize(peer, None));
    }
}
