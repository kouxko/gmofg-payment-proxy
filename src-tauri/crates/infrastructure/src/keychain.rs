//! macOS current-user secret protection backed by the login Keychain.

use std::sync::Mutex;

use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use security_framework::passwords::{PasswordOptions, generic_password, set_generic_password};
use zeroize::Zeroizing;

use crate::{InfrastructureError, SecretProtector};

const DEFAULT_SERVICE: &str = "com.gmofg.payment-proxy";
const DEFAULT_ACCOUNT: &str = "secret-protection-master-key-v1";
const ENVELOPE_MAGIC: &[u8; 5] = b"GMPK1";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const AAD: &[u8] = b"gmofg-payment-proxy/keychain-envelope/v1";

/// Uses a random AES-256-GCM master key stored in the current user's login
/// Keychain. `SQLite` only receives authenticated ciphertext envelopes.
#[derive(Debug)]
pub struct MacKeychainProtector {
    service: String,
    account: String,
    key_gate: Mutex<()>,
}

impl Default for MacKeychainProtector {
    fn default() -> Self {
        Self::new(DEFAULT_SERVICE, DEFAULT_ACCOUNT)
    }
}

impl MacKeychainProtector {
    #[must_use]
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
            key_gate: Mutex::new(()),
        }
    }

    fn master_key(&self) -> Result<Zeroizing<Vec<u8>>, InfrastructureError> {
        let _guard = self
            .key_gate
            .lock()
            .map_err(|_| InfrastructureError::KeychainProtect)?;
        if let Ok(key) = generic_password(PasswordOptions::new_generic_password(
            &self.service,
            &self.account,
        )) {
            return validate_key(key);
        }

        let mut key = Zeroizing::new(vec![0_u8; KEY_BYTES]);
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| InfrastructureError::KeychainProtect)?;
        set_generic_password(&self.service, &self.account, &key)
            .map_err(|_| InfrastructureError::KeychainProtect)?;
        Ok(key)
    }
}

impl SecretProtector for MacKeychainProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        let key = self
            .master_key()
            .map_err(|_| InfrastructureError::KeychainProtect)?;
        seal(&key, plaintext)
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        let key = self
            .master_key()
            .map_err(|_| InfrastructureError::KeychainUnprotect)?;
        open(&key, ciphertext)
    }
}

fn validate_key(key: Vec<u8>) -> Result<Zeroizing<Vec<u8>>, InfrastructureError> {
    if key.len() != KEY_BYTES {
        return Err(InfrastructureError::KeychainUnprotect);
    }
    Ok(Zeroizing::new(key))
}

fn cipher(key: &[u8], error: InfrastructureError) -> Result<LessSafeKey, InfrastructureError> {
    UnboundKey::new(&AES_256_GCM, key)
        .map(LessSafeKey::new)
        .map_err(|_| error)
}

fn seal(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
    let cipher = cipher(key, InfrastructureError::KeychainProtect)?;
    let mut nonce = [0_u8; NONCE_BYTES];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| InfrastructureError::KeychainProtect)?;
    let mut protected = plaintext.to_vec();
    cipher
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(AAD),
            &mut protected,
        )
        .map_err(|_| InfrastructureError::KeychainProtect)?;

    let mut envelope = Vec::with_capacity(ENVELOPE_MAGIC.len() + NONCE_BYTES + protected.len());
    envelope.extend_from_slice(ENVELOPE_MAGIC);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&protected);
    Ok(envelope)
}

fn open(key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
    let payload = ciphertext
        .strip_prefix(ENVELOPE_MAGIC)
        .ok_or(InfrastructureError::KeychainUnprotect)?;
    if payload.len() < NONCE_BYTES + TAG_BYTES {
        return Err(InfrastructureError::KeychainUnprotect);
    }
    let (nonce, encrypted) = payload.split_at(NONCE_BYTES);
    let nonce: [u8; NONCE_BYTES] = nonce
        .try_into()
        .map_err(|_| InfrastructureError::KeychainUnprotect)?;
    let mut protected = Zeroizing::new(encrypted.to_vec());
    let cipher = cipher(key, InfrastructureError::KeychainUnprotect)?;
    let plaintext = cipher
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(AAD),
            &mut protected,
        )
        .map_err(|_| InfrastructureError::KeychainUnprotect)?;
    Ok(plaintext.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trip_is_randomized_and_authenticated() {
        let key = [7_u8; KEY_BYTES];
        let first = seal(&key, b"secret").expect("seal");
        let second = seal(&key, b"secret").expect("seal");
        assert_ne!(first, second);
        assert_eq!(open(&key, &first).expect("open"), b"secret");

        let last = first.len() - 1;
        let mut tampered = first;
        tampered[last] ^= 1;
        assert!(matches!(
            open(&key, &tampered),
            Err(InfrastructureError::KeychainUnprotect)
        ));
    }

    #[test]
    fn malformed_envelopes_fail_closed() {
        let key = [9_u8; KEY_BYTES];
        assert!(open(&key, b"plaintext").is_err());
        assert!(open(&key, ENVELOPE_MAGIC).is_err());
        assert!(cipher(&key[..KEY_BYTES - 1], InfrastructureError::KeychainProtect).is_err());
    }
}
