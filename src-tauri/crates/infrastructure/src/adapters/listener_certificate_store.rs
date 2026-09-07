//! Listener TLS 材料的受保护封装格式与受限文件读取。

use std::{fs::File, io::Read, path::Path};

use intercept_proxy_application::{AppError, AppResult};
use zeroize::{Zeroize, Zeroizing};

pub(super) const REFERENCE_PREFIX: &str = "managed:listener-tls:";
pub(super) const FORMAT_VERSION: u8 = 1;
const MAX_IMPORT_BYTES: u64 = 16 * 1024 * 1024;

pub(super) struct ManagedMaterial {
    pub kind: u8,
    pub password: Zeroizing<Vec<u8>>,
    pub bytes: Zeroizing<Vec<u8>>,
}

pub(super) fn decode_material(mut plaintext: Zeroizing<Vec<u8>>) -> AppResult<ManagedMaterial> {
    if plaintext.len() < 6 || plaintext[0] != FORMAT_VERSION {
        return Err(AppError::new(
            "CERTIFICATE_NOT_READY",
            "Listener TLS 安全材料格式无效。",
        ));
    }
    let kind = plaintext[1];
    let password_len =
        u32::from_be_bytes(plaintext[2..6].try_into().expect("fixed slice")) as usize;
    if password_len > plaintext.len() - 6 {
        return Err(AppError::new(
            "CERTIFICATE_NOT_READY",
            "Listener TLS 安全材料已损坏。",
        ));
    }
    let mut password = Zeroizing::new(plaintext[6..6 + password_len].to_vec());
    let bytes = Zeroizing::new(plaintext.split_off(6 + password_len));
    plaintext.zeroize();
    Ok(ManagedMaterial {
        kind,
        password: std::mem::take(&mut password),
        bytes,
    })
}

pub(super) fn managed_key(reference: &str) -> Option<AppResult<&str>> {
    reference.strip_prefix(REFERENCE_PREFIX).map(|key| {
        if key.is_empty() {
            Err(AppError::new(
                "CERTIFICATE_NOT_READY",
                "Listener TLS 安全引用为空。",
            ))
        } else {
            Ok(key)
        }
    })
}

pub(super) fn read_secret_file(path: &Path) -> AppResult<Zeroizing<Vec<u8>>> {
    let metadata = path.metadata().map_err(|error| {
        AppError::new("IMPORT_READ_FAILED", format!("无法读取导入文件：{error}"))
    })?;
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err(AppError::new(
            "IMPORT_TOO_LARGE",
            "证书导入文件不能超过 16 MiB。",
        ));
    }
    let file = File::open(path).map_err(|error| {
        AppError::new("IMPORT_READ_FAILED", format!("无法打开导入文件：{error}"))
    })?;
    let capacity = usize::try_from(metadata.len()).expect("16 MiB import fits usize");
    let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
    file.take(MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            AppError::new("IMPORT_READ_FAILED", format!("无法读取导入文件：{error}"))
        })?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(AppError::new(
            "IMPORT_TOO_LARGE",
            "证书导入文件不能超过 16 MiB。",
        ));
    }
    Ok(bytes)
}
