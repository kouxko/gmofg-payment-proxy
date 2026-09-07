use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ProtectedMaterial {
    pub(super) revision: u64,
    pub(super) certificate_der: Vec<u8>,
    pub(super) private_key_der: Vec<u8>,
    pub(super) chain_der: Vec<Vec<u8>>,
    pub(super) subject: String,
    pub(super) fingerprint: String,
    pub(super) sans: Vec<String>,
    pub(super) not_before: String,
    pub(super) not_after: String,
}

pub(super) struct MaterialSnapshot {
    pub(super) revision: u64,
    pub(super) materials: BTreeMap<String, ProtectedMaterial>,
}

/// 可直接用于状态栏和列表的非敏感证书元数据。
///
/// 它刻意不包含证书 DER、私钥或保护后的密文，因此读取该结构不需要访问系统密钥库。
#[derive(Debug, Clone, Deserialize)]
pub(super) struct MaterialStatus {
    /// 单条证书记录的修订号；集合修订号推进时，未变化记录不会被重写。
    #[serde(rename = "revision")]
    pub(super) _revision: u64,
    pub(super) subject: String,
    pub(super) fingerprint: String,
    pub(super) sans: Vec<String>,
    pub(super) not_before: Option<String>,
    pub(super) not_after: Option<String>,
}

impl fmt::Debug for ProtectedMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtectedMaterial")
            .field("revision", &self.revision)
            .field("subject", &self.subject)
            .field("fingerprint", &self.fingerprint)
            .field("sans", &self.sans)
            .field("secret_material", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl Drop for ProtectedMaterial {
    fn drop(&mut self) {
        self.private_key_der.zeroize();
        for certificate in &mut self.chain_der {
            certificate.zeroize();
        }
    }
}
