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

use crate::{IntoSqlitePersistence, ProtectedSecretRecord, SecretProtector, SqliteExecutor};

use super::common::{app_error, infra};

const PROVIDER: &str = "system";

pub struct ProtectedSecretAdapter {
    executor: SqliteExecutor,
    protector: Arc<dyn SecretProtector>,
}

impl fmt::Debug for ProtectedSecretAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtectedSecretAdapter")
            .field("executor", &self.executor)
            .field("protector", &"<system secret protector>")
            .finish()
    }
}

impl ProtectedSecretAdapter {
    #[must_use]
    pub fn new(
        persistence: impl IntoSqlitePersistence,
        protector: Arc<dyn SecretProtector>,
    ) -> Self {
        let (executor, store) = persistence.into_sqlite_persistence();
        drop(store);
        Self {
            executor,
            protector,
        }
    }

    pub async fn resolve_basic_authenticator(
        &self,
        reference: &SecretReference,
    ) -> AppResult<Arc<dyn ForwardProxyAuthenticator>> {
        if reference.provider != PROVIDER || reference.key.trim().is_empty() {
            return Err(AppError::new(
                "SECRET_REFERENCE_INVALID",
                "代理认证安全引用无效或不属于当前安装实例。",
            ));
        }
        let provider = reference.provider.clone();
        let key = reference.key.clone();
        let record = self
            .executor
            .execute(move |store| {
                store
                    .load_protected_secret(&provider, &key)
                    .map_err(AppError::from)
            })
            .await?
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
        let record = ProtectedSecretRecord {
            provider: PROVIDER.into(),
            key: key.clone(),
            protected_blob,
            updated_at: Utc::now(),
        };
        self.executor
            .execute(move |store| store.save_protected_secret(&record))
            .await
            .map_err(app_error)?;
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
    use std::{future::Future, pin::Pin, sync::mpsc, task::Poll};

    use super::*;
    use crate::{InfrastructureError, SqliteStore};
    use tokio::sync::oneshot;

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

        let authenticator = adapter
            .resolve_basic_authenticator(&reference)
            .await
            .unwrap();
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

    #[tokio::test(flavor = "current_thread")]
    async fn basic_auth_resolution_waits_asynchronously_and_queued_cancel_is_safe() {
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let adapter = ProtectedSecretAdapter::new(store, Arc::new(TestProtector));
        let reference = adapter
            .store_basic_auth("operator".into(), "secret".into())
            .await
            .unwrap();
        let executor = adapter.executor.clone();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let blocker = tokio::spawn(async move {
            executor
                .execute(move |_| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok::<_, InfrastructureError>(())
                })
                .await
        });
        entered_rx.await.unwrap();

        let mut resolution = Box::pin(adapter.resolve_basic_authenticator(&reference));
        let polled =
            std::future::poll_fn(|context| Poll::Ready(Pin::new(&mut resolution).poll(context)))
                .await;
        assert!(matches!(polled, Poll::Pending));
        let (progress_tx, progress_rx) = oneshot::channel();
        tokio::spawn(async move { progress_tx.send(()).unwrap() });
        progress_rx.await.unwrap();
        drop(resolution);

        release_tx.send(()).unwrap();
        blocker.await.unwrap().unwrap();
        adapter
            .resolve_basic_authenticator(&reference)
            .await
            .expect("later resolution succeeds");
    }
}
