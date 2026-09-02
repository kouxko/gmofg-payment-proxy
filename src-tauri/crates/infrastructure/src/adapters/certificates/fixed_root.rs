//! 可选固定 Root CA 的装载与安装实例证书签发。
//!
//! 固定 Root 由产品策略显式提供；未提供时为当前安装实例生成独立 Root。两条路径
//! 共用相同的本地叶子证书签发与存储流程。

use std::io::Cursor;

use intercept_proxy_application::{AppError, AppResult};

use super::{
    CertificateServiceAdapter, LEAF, MaterialSnapshot, ROOT, app_error, from_bundle, leaf_request,
};

const REVOKED_SHARED_TEST_ROOT_CA_PEM: &[u8] =
    include_bytes!("../../../../../resources/certificates/intercept-proxy-test-root-ca.crt");

impl CertificateServiceAdapter {
    pub(super) fn is_revoked_shared_test_root(certificate_der: &[u8]) -> AppResult<bool> {
        let mut reader = Cursor::new(REVOKED_SHARED_TEST_ROOT_CA_PEM);
        let mut certificates = rustls_pemfile::certs(&mut reader);
        let revoked = certificates
            .next()
            .transpose()
            .map_err(|error| {
                AppError::new(
                    "CERTIFICATE_INVALID",
                    format!("已撤销 Root CA 检测资源解析失败：{error}"),
                )
            })?
            .ok_or_else(|| AppError::new("CERTIFICATE_INVALID", "已撤销 Root CA 检测资源为空。"))?;
        if certificates.next().is_some() {
            return Err(AppError::new(
                "CERTIFICATE_INVALID",
                "已撤销 Root CA 检测资源必须只包含一张证书。",
            ));
        }
        Ok(certificate_der == revoked.as_ref())
    }

    pub(super) fn reject_revoked_shared_test_root(certificate_der: &[u8]) -> AppResult<()> {
        if Self::is_revoked_shared_test_root(certificate_der)? {
            return Err(AppError::new(
                "CERTIFICATE_ROOT_REVOKED",
                "检测到已撤销的旧共享测试 Root CA。请清除全部配置与数据，并从客户端和系统信任库删除旧 Root 后重新初始化。",
            ));
        }
        Ok(())
    }

    pub(super) fn fixed_root_bundle(&self) -> AppResult<Option<crate::CertificateBundle>> {
        match (
            self.certificate_policy().fixed_installation_root_ca_pem(),
            self.certificate_policy().fixed_installation_root_key_pem(),
        ) {
            (Some(certificate), Some(private_key)) => self
                .certificates
                .load_fixed_root_ca(certificate, private_key)
                .map(Some)
                .map_err(app_error),
            (None, None) => Ok(None),
            _ => Err(AppError::new(
                "CERTIFICATE_INVALID",
                "固定 Root CA 公共证书与签发私钥必须成对配置。",
            )),
        }
    }

    pub(super) async fn generate_async(
        &self,
        sans: &[String],
        mut snapshot: MaterialSnapshot,
    ) -> AppResult<u64> {
        let root = match self.fixed_root_bundle()? {
            Some(root) => root,
            None => self
                .certificates
                .generate_root_ca(self.certificate_policy().labels().root_name)
                .map_err(app_error)?,
        };
        let request = leaf_request(sans)?;
        let leaf = self
            .certificates
            .generate_leaf(&root.certificate_der, &root.private_key_pkcs8_der, &request)
            .map_err(app_error)?;
        snapshot
            .materials
            .insert(ROOT.into(), from_bundle(snapshot.revision, &root));
        snapshot
            .materials
            .insert(LEAF.into(), from_bundle(snapshot.revision, &leaf));
        self.commit_snapshot_async(snapshot).await
    }
}
