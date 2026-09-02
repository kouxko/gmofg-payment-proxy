//! 固定测试 Root CA 的装载与本地叶子证书签发。
//!
//! 单独放在该模块中，是为了让“跨平台共享 Root”这一产品策略与证书存储、导入等
//! 通用流程保持边界清晰。Windows 与 macOS 使用完全相同的固定 Root；每台机器仍会
//! 根据自己的监听地址重新签发叶子证书。

use intercept_proxy_application::{AppError, AppResult};

use super::{
    CertificateServiceAdapter, LEAF, MaterialSnapshot, ROOT, app_error, from_bundle, leaf_request,
};

impl CertificateServiceAdapter {
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
