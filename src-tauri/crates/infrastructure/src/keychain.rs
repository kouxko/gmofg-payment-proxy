//! macOS current-user secret protection backed by the login Keychain.

use std::{fmt, sync::Mutex};

use gmofg_proxy_product_api::ProductStorageNamespace;
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use security_framework::{
    os::macos::keychain::SecKeychain,
    passwords::{PasswordOptions, generic_password},
};
use zeroize::Zeroizing;

use crate::{InfrastructureError, SecretProtector};

const DEFAULT_SERVICE: &str = "com.generic-proxy";
const DEFAULT_ACCOUNT: &str = "secret-protection-master-key-v1";
const DEFAULT_ENVELOPE_MAGIC: &[u8; 5] = b"GPXK1";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const DEFAULT_AAD: &[u8] = b"generic-proxy/keychain-envelope/v1";
const ERR_SEC_DUPLICATE_ITEM: i32 = -25_299;
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25_300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyStoreError {
    NotFound,
    Duplicate,
    Other,
}

trait MasterKeyStore: fmt::Debug + Send + Sync {
    fn read(&self, service: &str, account: &str) -> Result<Vec<u8>, KeyStoreError>;
    fn add(&self, service: &str, account: &str, key: &[u8]) -> Result<(), KeyStoreError>;
}

#[derive(Debug)]
struct SystemKeyStore;

impl MasterKeyStore for SystemKeyStore {
    fn read(&self, service: &str, account: &str) -> Result<Vec<u8>, KeyStoreError> {
        generic_password(PasswordOptions::new_generic_password(service, account))
            .map_err(|error| classify_status(error.code()))
    }

    fn add(&self, service: &str, account: &str, key: &[u8]) -> Result<(), KeyStoreError> {
        SecKeychain::default()
            .map_err(|error| classify_status(error.code()))?
            .add_generic_password(service, account, key)
            .map_err(|error| classify_status(error.code()))
    }
}

const fn classify_status(status: i32) -> KeyStoreError {
    match status {
        ERR_SEC_ITEM_NOT_FOUND => KeyStoreError::NotFound,
        ERR_SEC_DUPLICATE_ITEM => KeyStoreError::Duplicate,
        _ => KeyStoreError::Other,
    }
}

/// Uses a random AES-256-GCM master key stored in the current user's login
/// Keychain. `SQLite` only receives authenticated ciphertext envelopes.
#[derive(Debug)]
pub struct MacKeychainProtector {
    service: String,
    account: String,
    envelope_magic: [u8; 5],
    aad: Vec<u8>,
    key_store: Box<dyn MasterKeyStore>,
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
            envelope_magic: *DEFAULT_ENVELOPE_MAGIC,
            aad: DEFAULT_AAD.to_vec(),
            key_store: Box::new(SystemKeyStore),
            key_gate: Mutex::new(()),
        }
    }

    #[must_use]
    pub fn for_namespace(namespace: ProductStorageNamespace) -> Self {
        Self {
            service: namespace.secret_service.into(),
            account: namespace.secret_account.into(),
            envelope_magic: *namespace.secret_envelope_magic,
            aad: namespace.secret_aad.to_vec(),
            key_store: Box::new(SystemKeyStore),
            key_gate: Mutex::new(()),
        }
    }

    #[cfg(test)]
    fn with_key_store(key_store: impl MasterKeyStore + 'static) -> Self {
        Self {
            service: DEFAULT_SERVICE.into(),
            account: DEFAULT_ACCOUNT.into(),
            envelope_magic: *DEFAULT_ENVELOPE_MAGIC,
            aad: DEFAULT_AAD.to_vec(),
            key_store: Box::new(key_store),
            key_gate: Mutex::new(()),
        }
    }

    fn master_key(&self) -> Result<Zeroizing<Vec<u8>>, InfrastructureError> {
        let _guard = self
            .key_gate
            .lock()
            .map_err(|_| InfrastructureError::KeychainProtect)?;
        match self.key_store.read(&self.service, &self.account) {
            Ok(key) => return validate_key(key),
            Err(KeyStoreError::NotFound) => {}
            Err(KeyStoreError::Duplicate | KeyStoreError::Other) => {
                return Err(InfrastructureError::KeychainUnprotect);
            }
        }

        let mut key = Zeroizing::new(vec![0_u8; KEY_BYTES]);
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| InfrastructureError::KeychainProtect)?;
        match self.key_store.add(&self.service, &self.account, &key) {
            Ok(()) => Ok(key),
            Err(KeyStoreError::Duplicate) => self
                .key_store
                .read(&self.service, &self.account)
                .map_err(|_| InfrastructureError::KeychainUnprotect)
                .and_then(validate_key),
            Err(KeyStoreError::NotFound | KeyStoreError::Other) => {
                Err(InfrastructureError::KeychainProtect)
            }
        }
    }
}

impl SecretProtector for MacKeychainProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        let key = self
            .master_key()
            .map_err(|_| InfrastructureError::KeychainProtect)?;
        seal(&key, self.envelope_magic, &self.aad, plaintext)
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        let key = self
            .master_key()
            .map_err(|_| InfrastructureError::KeychainUnprotect)?;
        open(&key, self.envelope_magic, &self.aad, ciphertext)
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

fn seal(
    key: &[u8],
    envelope_magic: [u8; 5],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, InfrastructureError> {
    let cipher = cipher(key, InfrastructureError::KeychainProtect)?;
    let mut nonce = [0_u8; NONCE_BYTES];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| InfrastructureError::KeychainProtect)?;
    let mut protected = Zeroizing::new(plaintext.to_vec());
    cipher
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut *protected,
        )
        .map_err(|_| InfrastructureError::KeychainProtect)?;

    let mut envelope = Vec::with_capacity(envelope_magic.len() + NONCE_BYTES + protected.len());
    envelope.extend_from_slice(&envelope_magic);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&protected);
    Ok(envelope)
}

fn open(
    key: &[u8],
    envelope_magic: [u8; 5],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, InfrastructureError> {
    let payload = ciphertext
        .strip_prefix(&envelope_magic)
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
            Aad::from(aad),
            &mut protected,
        )
        .map_err(|_| InfrastructureError::KeychainUnprotect)?;
    Ok(plaintext.to_vec())
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

    use super::*;

    #[test]
    fn product_namespace_controls_keychain_and_envelope_context() {
        let protector = MacKeychainProtector::for_namespace(ProductStorageNamespace {
            database_file_name: "ignored.sqlite3",
            secret_service: "com.example.alpha",
            secret_account: "alpha-key",
            secret_envelope_magic: b"ALPK1",
            secret_aad: b"alpha/envelope/v1",
        });
        assert_eq!(protector.service, "com.example.alpha");
        assert_eq!(protector.account, "alpha-key");
        assert_eq!(&protector.envelope_magic, b"ALPK1");
        assert_eq!(protector.aad, b"alpha/envelope/v1");
    }

    #[derive(Debug, Clone)]
    struct FakeKeyStore {
        state: Arc<Mutex<FakeKeyStoreState>>,
    }

    #[derive(Debug)]
    struct FakeKeyStoreState {
        reads: VecDeque<Result<Vec<u8>, KeyStoreError>>,
        adds: Vec<Vec<u8>>,
        add_result: Result<(), KeyStoreError>,
    }

    impl FakeKeyStore {
        fn new(
            reads: impl IntoIterator<Item = Result<Vec<u8>, KeyStoreError>>,
            add_result: Result<(), KeyStoreError>,
        ) -> Self {
            Self {
                state: Arc::new(Mutex::new(FakeKeyStoreState {
                    reads: reads.into_iter().collect(),
                    adds: Vec::new(),
                    add_result,
                })),
            }
        }

        fn add_count(&self) -> usize {
            self.state.lock().expect("fake key store").adds.len()
        }

        fn added_key(&self) -> Vec<u8> {
            self.state.lock().expect("fake key store").adds[0].clone()
        }

        fn remaining_reads(&self) -> usize {
            self.state.lock().expect("fake key store").reads.len()
        }
    }

    impl MasterKeyStore for FakeKeyStore {
        fn read(&self, _: &str, _: &str) -> Result<Vec<u8>, KeyStoreError> {
            self.state
                .lock()
                .expect("fake key store")
                .reads
                .pop_front()
                .expect("unexpected read")
        }

        fn add(&self, _: &str, _: &str, key: &[u8]) -> Result<(), KeyStoreError> {
            let mut state = self.state.lock().expect("fake key store");
            state.adds.push(key.to_vec());
            state.add_result
        }
    }

    #[test]
    fn not_found_creates_a_new_master_key() {
        let store = FakeKeyStore::new([Err(KeyStoreError::NotFound)], Ok(()));
        let protector = MacKeychainProtector::with_key_store(store.clone());

        let key = protector.master_key().expect("master key");

        assert_eq!(store.add_count(), 1);
        assert_eq!(key.as_slice(), store.added_key());
        assert_eq!(key.len(), KEY_BYTES);
    }

    #[test]
    fn ordinary_read_error_fails_closed_without_writing() {
        let store = FakeKeyStore::new([Err(KeyStoreError::Other)], Ok(()));
        let protector = MacKeychainProtector::with_key_store(store.clone());

        assert!(matches!(
            protector.protect(b"secret"),
            Err(InfrastructureError::KeychainProtect)
        ));
        assert_eq!(store.add_count(), 0);
    }

    #[test]
    fn existing_key_is_used_without_writing() {
        let existing = vec![3_u8; KEY_BYTES];
        let store = FakeKeyStore::new([Ok(existing.clone())], Ok(()));
        let protector = MacKeychainProtector::with_key_store(store.clone());

        assert_eq!(
            protector.master_key().expect("master key").as_slice(),
            existing
        );
        assert_eq!(store.add_count(), 0);
    }

    #[test]
    fn invalid_existing_key_fails_closed_without_writing() {
        let store = FakeKeyStore::new([Ok(vec![3_u8; KEY_BYTES - 1])], Ok(()));
        let protector = MacKeychainProtector::with_key_store(store.clone());

        assert!(matches!(
            protector.unprotect(b"invalid envelope"),
            Err(InfrastructureError::KeychainUnprotect)
        ));
        assert_eq!(store.add_count(), 0);
    }

    #[test]
    fn duplicate_add_rereads_the_concurrently_created_key() {
        let concurrent = vec![5_u8; KEY_BYTES];
        let store = FakeKeyStore::new(
            [Err(KeyStoreError::NotFound), Ok(concurrent.clone())],
            Err(KeyStoreError::Duplicate),
        );
        let protector = MacKeychainProtector::with_key_store(store.clone());

        assert_eq!(
            protector.master_key().expect("master key").as_slice(),
            concurrent
        );
        assert_eq!(store.add_count(), 1);
        assert_eq!(store.remaining_reads(), 0);
    }

    #[test]
    fn envelope_round_trip_is_randomized_and_authenticated() {
        let key = [7_u8; KEY_BYTES];
        let first = seal(&key, *DEFAULT_ENVELOPE_MAGIC, DEFAULT_AAD, b"secret").expect("seal");
        let second = seal(&key, *DEFAULT_ENVELOPE_MAGIC, DEFAULT_AAD, b"secret").expect("seal");
        assert_ne!(first, second);
        assert_eq!(
            open(&key, *DEFAULT_ENVELOPE_MAGIC, DEFAULT_AAD, &first).expect("open"),
            b"secret"
        );

        let last = first.len() - 1;
        let mut tampered = first;
        tampered[last] ^= 1;
        assert!(matches!(
            open(&key, *DEFAULT_ENVELOPE_MAGIC, DEFAULT_AAD, &tampered),
            Err(InfrastructureError::KeychainUnprotect)
        ));
    }

    #[test]
    fn malformed_envelopes_fail_closed() {
        let key = [9_u8; KEY_BYTES];
        assert!(open(&key, *DEFAULT_ENVELOPE_MAGIC, DEFAULT_AAD, b"plaintext").is_err());
        assert!(
            open(
                &key,
                *DEFAULT_ENVELOPE_MAGIC,
                DEFAULT_AAD,
                DEFAULT_ENVELOPE_MAGIC
            )
            .is_err()
        );
        assert!(cipher(&key[..KEY_BYTES - 1], InfrastructureError::KeychainProtect).is_err());
    }
}
