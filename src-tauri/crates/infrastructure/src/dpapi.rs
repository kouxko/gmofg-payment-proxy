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
        unsafe { slice::from_raw_parts(blob.0.pbData, blob.0.cbData as usize).to_vec() }
    }

    impl SecretProtector for DpapiProtector {
        fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            let mut input = input_blob(plaintext)?;
            let mut output = LocalBlob(CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: ptr::null_mut(),
            });
            let ok = unsafe {
                CryptProtectData(
                    &mut input,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
                )
            };
            if ok == 0 {
                return Err(InfrastructureError::DpapiProtect);
            }
            Ok(copy_output(&output))
        }

        fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            let mut input =
                input_blob(ciphertext).map_err(|_| InfrastructureError::DpapiUnprotect)?;
            let mut output = LocalBlob(CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: ptr::null_mut(),
            });
            let ok = unsafe {
                CryptUnprotectData(
                    &mut input,
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output.0,
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
