use std::{fmt, io::Cursor};

use rcgen::KeyPair;
use zeroize::{Zeroize, Zeroizing};

use super::{CertificateMetadata, invalid, rcgen_error, x509_error};
use crate::InfrastructureError;

/// 已校验并转换为 PKCS#8 的 PEM 身份。
///
/// Listener 运行时统一按 PKCS#8 向 rustls 提交私钥，因此导入阶段即完成格式归一化，
/// 避免把只能在当前临时文件存在时才能解析的外部路径保存进 Workspace。
pub struct ParsedPemIdentity {
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub private_key_pkcs8_der: Zeroizing<Vec<u8>>,
    pub metadata: CertificateMetadata,
}

pub(super) struct PemIdentityMaterial {
    pub(super) certificate_chain_der: Vec<Vec<u8>>,
    pub(super) private_key_pkcs8_der: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for ParsedPemIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParsedPemIdentity")
            .field("certificate_chain_len", &self.certificate_chain_der.len())
            .field("private_key_pkcs8_der", &"<redacted>")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl Drop for ParsedPemIdentity {
    fn drop(&mut self) {
        for certificate in &mut self.certificate_chain_der {
            certificate.zeroize();
        }
    }
}

pub(super) fn parse_pem_identity(
    bytes: &[u8],
    label: &str,
) -> Result<PemIdentityMaterial, InfrastructureError> {
    let mut certificates = Cursor::new(bytes);
    let certificate_chain_der = rustls_pemfile::certs(&mut certificates)
        .map(|entry| {
            entry
                .map(|certificate| certificate.as_ref().to_vec())
                .map_err(x509_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if certificate_chain_der.is_empty() {
        return Err(invalid(format!("{label}缺少证书链")));
    }
    let mut private_key = Cursor::new(bytes);
    let private_key = rustls_pemfile::private_key(&mut private_key)
        .map_err(x509_error)?
        .ok_or_else(|| invalid(format!("{label}缺少私钥")))?;
    let key_pair = KeyPair::try_from(&private_key).map_err(rcgen_error)?;
    Ok(PemIdentityMaterial {
        certificate_chain_der,
        private_key_pkcs8_der: Zeroizing::new(key_pair.serialize_der()),
    })
}
