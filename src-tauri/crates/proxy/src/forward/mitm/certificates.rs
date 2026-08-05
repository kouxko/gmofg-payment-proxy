//! MITM 叶子证书签发、TLS `ServerConfig` 构建与有界缓存。

use super::{ForwardMitmRuntime, Result, tls_config_error};
use crate::{ErrorCode, ProxyError};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::sync::Arc;

impl ForwardMitmRuntime {
    pub(in crate::forward::service) async fn server_config_for(
        &self,
        authority_host: &str,
    ) -> Result<Arc<ServerConfig>> {
        let cache_key = authority_host.to_ascii_lowercase();
        if let Some(config) = self.leaf_cache.lock().await.get(&cache_key) {
            return Ok(config);
        }

        // 签发可能涉及解密受保护 Root 私钥，不能持有异步缓存锁跨越该边界。允许极少量
        // 同 authority 并发首请求重复签发，最终只缓存一个，避免全局连接队头阻塞。
        let identity = self
            .certificate_authority
            .issue_server_identity(authority_host)?;
        if identity.certificate_chain_der.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::CertificateInvalid,
                "MITM leaf certificate chain is empty",
            ));
        }
        let certificates = identity
            .certificate_chain_der
            .into_iter()
            .map(CertificateDer::from)
            .collect();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            identity.private_key_pkcs8_der.to_vec(),
        ));
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(tls_config_error)?
                .with_no_client_auth()
                .with_single_cert(certificates, key)
                .map_err(tls_config_error)?;
        let config = Arc::new(config);
        let mut cache = self.leaf_cache.lock().await;
        if let Some(existing) = cache.get(&cache_key) {
            return Ok(existing);
        }
        cache.insert(&cache_key, &config);
        Ok(config)
    }
}
