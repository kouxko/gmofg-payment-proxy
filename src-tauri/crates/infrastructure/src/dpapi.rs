//! 跨平台的敏感数据保护接口，以及 Windows 当前用户级 DPAPI 实现。
//!
//! `SecretProtector` 只负责“把字节交给操作系统保护/恢复”，不负责持久化；非 Windows
//! 平台会明确返回不支持，而不是退化为明文。Windows 返回的系统缓冲区由 RAII 包装，
//! 离开作用域时先清零再释放，失败信息也不会包含密钥或证书内容。

use crate::InfrastructureError;

/// Current-user secret protection boundary.
pub trait SecretProtector: Send + Sync {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError>;
    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError>;
}

/// Windows DPAPI protector. Machine-scope protection is intentionally never
/// requested, so ciphertext is bound to the current Windows user.
#[derive(Debug, Default, Clone, Copy)]
pub struct DpapiProtector;

#[cfg(not(windows))]
impl SecretProtector for DpapiProtector {
    fn protect(&self, _plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Err(InfrastructureError::DpapiUnsupported)
    }

    fn unprotect(&self, _ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Err(InfrastructureError::DpapiUnsupported)
    }
}

#[cfg(windows)]
mod windows_impl {
    #![allow(unsafe_code)]

    use std::{ptr, slice};

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };

    use super::{DpapiProtector, SecretProtector};
    use crate::InfrastructureError;

    struct LocalBlob(CRYPT_INTEGER_BLOB);

    impl Drop for LocalBlob {
        fn drop(&mut self) {
            if self.0.pbData.is_null() {
                return;
            }
            unsafe {
                // SAFETY: DPAPI returned this writable buffer and `cbData` byte length. The
                // wrapper owns it until this `Drop`, so zeroing then releasing it exactly once is
                // valid. `LocalFree` accepts the allocation returned by DPAPI.
                ptr::write_bytes(self.0.pbData, 0, self.0.cbData as usize);
                LocalFree(self.0.pbData.cast());
            }
        }
    }

    fn input_blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, InfrastructureError> {
        let length = u32::try_from(bytes.len()).map_err(|_| InfrastructureError::DpapiProtect)?;
        Ok(CRYPT_INTEGER_BLOB {
            cbData: length,
            pbData: bytes.as_ptr().cast_mut(),
        })
    }

    fn copy_output(blob: &LocalBlob) -> Vec<u8> {
        unsafe {
            // SAFETY: a successful DPAPI call initializes `pbData` with exactly `cbData` bytes.
            // `blob` remains alive for the duration of this copy and owns the allocation.
            slice::from_raw_parts(blob.0.pbData, blob.0.cbData as usize).to_vec()
        }
    }

    impl SecretProtector for DpapiProtector {
        fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            let input = input_blob(plaintext)?;
            let mut output = LocalBlob(CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: ptr::null_mut(),
            });
            let ok = unsafe {
                // SAFETY: all pointers follow the Windows DPAPI contract. `input` borrows
                // `plaintext` for this call, optional parameters are null, and `output` is a
                // writable zero-initialized blob subsequently owned by `LocalBlob`.
                CryptProtectData(
                    &raw const input,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &raw mut output.0,
                )
            };
            if ok == 0 {
                return Err(InfrastructureError::DpapiProtect);
            }
            Ok(copy_output(&output))
        }

        fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            let input = input_blob(ciphertext).map_err(|_| InfrastructureError::DpapiUnprotect)?;
            let mut output = LocalBlob(CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: ptr::null_mut(),
            });
            let ok = unsafe {
                // SAFETY: all pointers follow the Windows DPAPI contract. `input` borrows
                // `ciphertext` for this call, optional parameters are null, and `output` is a
                // writable zero-initialized blob subsequently owned by `LocalBlob`.
                CryptUnprotectData(
                    &raw const input,
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &raw mut output.0,
                )
            };
            if ok == 0 {
                return Err(InfrastructureError::DpapiUnprotect);
            }
            Ok(copy_output(&output))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SECURITY-006, SECURITY-007: non-Windows builds must never provide a
    /// plaintext fallback for DPAPI.
    #[cfg(not(windows))]
    #[test]
    fn unsupported_platform_fails_closed() {
        let protector = DpapiProtector;
        assert!(matches!(
            protector.protect(b"secret"),
            Err(InfrastructureError::DpapiUnsupported)
        ));
    }

    /// SECURITY-006: Windows round-trip uses current-user DPAPI.
    #[cfg(windows)]
    #[test]
    fn current_user_round_trip() {
        let protector = DpapiProtector;
        let encrypted = protector.protect(b"secret").expect("protect");
        assert_ne!(encrypted, b"secret");
        assert_eq!(
            protector.unprotect(&encrypted).expect("unprotect"),
            b"secret"
        );
    }
}
