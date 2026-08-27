//! 下游动态服务端证书解析与有界缓存。
//!
//! 同一 SNI 共享一次签发结果（包括失败）；不同 SNI 可并行签发，但总数受限。

use std::{
    collections::{BTreeMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, MutexGuard},
};

use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use zeroize::Zeroize;

use super::ReverseClientIdentity;
use crate::{
    ErrorCode, MitmCertificateAuthority, ProxyError, Result, forward::authority_is_allowed,
};

const MAX_DYNAMIC_SERVER_IDENTITIES: usize = 256;
const MAX_DYNAMIC_IDENTITY_FLIGHTS: usize = 32;

#[derive(Debug)]
pub(super) struct DynamicServerIdentityResolver {
    authority: Arc<dyn MitmCertificateAuthority>,
    allowlist: Vec<String>,
    fallback: Arc<CertifiedKey>,
    state: Mutex<DynamicIdentityState>,
}

impl DynamicServerIdentityResolver {
    pub(super) fn new(
        authority: Arc<dyn MitmCertificateAuthority>,
        allowlist: Vec<String>,
        fallback: Arc<CertifiedKey>,
    ) -> Self {
        Self {
            authority,
            allowlist,
            fallback,
            state: Mutex::new(DynamicIdentityState::default()),
        }
    }

    fn resolve_identity(&self, server_name: &str) -> Result<Arc<CertifiedKey>> {
        let cache_key = server_name.to_ascii_lowercase();
        let (flight, owner) = match self.cached_or_flight(&cache_key)? {
            CacheLookup::Cached(value) => return Ok(value),
            CacheLookup::Owner(flight) => (flight, true),
            CacheLookup::Waiter(flight) => (flight, false),
        };
        if !owner {
            return flight.wait();
        }

        let mut owner = FlightOwner::new(self, cache_key, flight);
        let issued = catch_unwind(AssertUnwindSafe(|| {
            self.authority
                .issue_server_identity(server_name)
                .and_then(|identity| {
                    certified_key_from_parts(
                        &identity.certificate_chain_der,
                        identity.private_key_pkcs8_der.to_vec(),
                    )
                })
                .map(Arc::new)
        }))
        .unwrap_or_else(|_| {
            Err(ProxyError::new(
                ErrorCode::Internal,
                "dynamic server identity issuance panicked",
            ))
        });
        let outcome = SharedOutcome::from_result(issued);
        owner.complete(&outcome);
        outcome.into_result()
    }

    fn cached_or_flight(&self, key: &str) -> Result<CacheLookup> {
        let mut state = lock_recover(&self.state);
        if let Some(value) = state.cache.get(key) {
            return Ok(CacheLookup::Cached(value));
        }
        if let Some(flight) = state.in_flight.get(key) {
            return Ok(CacheLookup::Waiter(Arc::clone(flight)));
        }
        if state.in_flight.len() >= MAX_DYNAMIC_IDENTITY_FLIGHTS {
            return Err(ProxyError::new(
                ErrorCode::OperationInProgress,
                format!(
                    "dynamic server identity issuance limit reached ({MAX_DYNAMIC_IDENTITY_FLIGHTS})"
                ),
            ));
        }
        let flight = Arc::new(IdentityFlight::default());
        state.in_flight.insert(key.to_owned(), Arc::clone(&flight));
        Ok(CacheLookup::Owner(flight))
    }

    fn finish_flight(
        &self,
        key: &str,
        owner: &Arc<IdentityFlight>,
        value: Option<Arc<CertifiedKey>>,
    ) {
        let mut state = lock_recover(&self.state);
        let owner_matches = state
            .in_flight
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, owner));
        if !owner_matches {
            return;
        }
        if let Some(value) = value {
            state.cache.insert(key.to_owned(), value);
        }
        state.in_flight.remove(key);
    }
}

struct FlightOwner<'a> {
    resolver: &'a DynamicServerIdentityResolver,
    key: String,
    flight: Arc<IdentityFlight>,
    completed: bool,
}

impl<'a> FlightOwner<'a> {
    fn new(
        resolver: &'a DynamicServerIdentityResolver,
        key: String,
        flight: Arc<IdentityFlight>,
    ) -> Self {
        Self {
            resolver,
            key,
            flight,
            completed: false,
        }
    }

    fn complete(&mut self, outcome: &SharedOutcome) {
        // Publish before removing the exact owner so a concurrent waiter cannot start a second
        // issuance in the gap.
        self.flight.publish(outcome.clone());
        self.resolver
            .finish_flight(&self.key, &self.flight, outcome.value());
        self.completed = true;
    }
}

impl Drop for FlightOwner<'_> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let failure = SharedOutcome::Failure(Arc::new(SharedError {
            code: ErrorCode::Internal.as_str(),
            message: "dynamic server identity issuance owner was dropped".into(),
        }));
        self.flight.publish(failure);
        self.resolver.finish_flight(&self.key, &self.flight, None);
    }
}

impl ResolvesServerCert for DynamicServerIdentityResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let Some(server_name) = client_hello.server_name() else {
            tracing::info!("downstream ClientHello has no SNI; using fallback identity");
            return Some(Arc::clone(&self.fallback));
        };
        if !authority_is_allowed(server_name, &self.allowlist) {
            tracing::warn!(
                server_name,
                allowlist = ?self.allowlist,
                "downstream ClientHello SNI is not allowed"
            );
            return None;
        }
        tracing::info!(server_name, "resolving downstream server identity");
        match self.resolve_identity(server_name) {
            Ok(identity) => Some(identity),
            Err(error) => {
                tracing::error!(
                    error_code = error.code,
                    server_name,
                    error = %error,
                    "dynamic server identity resolution failed"
                );
                None
            }
        }
    }
}

enum CacheLookup {
    Cached(Arc<CertifiedKey>),
    Owner(Arc<IdentityFlight>),
    Waiter(Arc<IdentityFlight>),
}

#[derive(Debug, Default)]
struct IdentityFlight {
    outcome: Mutex<Option<SharedOutcome>>,
    ready: Condvar,
    #[cfg(test)]
    waiter_count: Mutex<usize>,
    #[cfg(test)]
    waiter_ready: Condvar,
}

impl IdentityFlight {
    fn wait(&self) -> Result<Arc<CertifiedKey>> {
        #[cfg(test)]
        {
            *lock_recover(&self.waiter_count) += 1;
            self.waiter_ready.notify_all();
        }
        let mut outcome = lock_recover(&self.outcome);
        while outcome.is_none() {
            outcome = self
                .ready
                .wait(outcome)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        outcome
            .as_ref()
            .expect("flight outcome is ready")
            .clone()
            .into_result()
    }

    #[cfg(test)]
    fn wait_for_waiters(&self, expected: usize) {
        let mut count = lock_recover(&self.waiter_count);
        while *count < expected {
            count = self
                .waiter_ready
                .wait(count)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn publish(&self, outcome: SharedOutcome) {
        *lock_recover(&self.outcome) = Some(outcome);
        self.ready.notify_all();
    }
}

#[derive(Clone, Debug)]
enum SharedOutcome {
    Success(Arc<CertifiedKey>),
    Failure(Arc<SharedError>),
}

impl SharedOutcome {
    fn from_result(result: Result<Arc<CertifiedKey>>) -> Self {
        match result {
            Ok(value) => Self::Success(value),
            Err(error) => Self::Failure(Arc::new(SharedError {
                code: error.code,
                message: error.message,
            })),
        }
    }

    fn value(&self) -> Option<Arc<CertifiedKey>> {
        match self {
            Self::Success(value) => Some(Arc::clone(value)),
            Self::Failure(_) => None,
        }
    }

    fn into_result(self) -> Result<Arc<CertifiedKey>> {
        match self {
            Self::Success(value) => Ok(value),
            Self::Failure(error) => Err(error.to_error()),
        }
    }
}

#[derive(Debug)]
struct SharedError {
    code: &'static str,
    message: String,
}

impl SharedError {
    fn to_error(&self) -> ProxyError {
        ProxyError {
            code: self.code,
            message: self.message.clone(),
        }
    }
}

#[derive(Debug, Default)]
struct DynamicIdentityState {
    cache: DynamicServerIdentityCache<CertifiedKey>,
    in_flight: BTreeMap<String, Arc<IdentityFlight>>,
}

#[derive(Debug)]
struct DynamicServerIdentityCache<T> {
    entries: BTreeMap<String, Arc<T>>,
    recency: VecDeque<String>,
}

impl<T> Default for DynamicServerIdentityCache<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            recency: VecDeque::new(),
        }
    }
}

impl<T> DynamicServerIdentityCache<T> {
    fn get(&mut self, key: &str) -> Option<Arc<T>> {
        let value = Arc::clone(self.entries.get(key)?);
        self.touch(key);
        Some(value)
    }

    fn insert(&mut self, key: String, value: Arc<T>) {
        self.recency.retain(|candidate| candidate != &key);
        while self.entries.len() >= MAX_DYNAMIC_SERVER_IDENTITIES {
            let Some(oldest) = self.recency.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.recency.push_back(key.clone());
        self.entries.insert(key, value);
    }

    fn touch(&mut self, key: &str) {
        self.recency.retain(|candidate| candidate != key);
        self.recency.push_back(key.to_owned());
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) fn certified_key(identity: &ReverseClientIdentity) -> Result<Arc<CertifiedKey>> {
    certified_key_from_parts(
        &identity.certificate_chain_der,
        identity.private_key_pkcs8_der.to_vec(),
    )
    .map(Arc::new)
}

fn certified_key_from_parts(
    certificate_chain_der: &[Vec<u8>],
    private_key_pkcs8_der: Vec<u8>,
) -> Result<CertifiedKey> {
    let mut private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key_pkcs8_der));
    let signing_key = rustls::crypto::ring::sign::any_supported_type(&private_key)
        .map_err(super::config_error)?;
    private_key.zeroize();
    let certificate_chain = certificate_chain_der
        .iter()
        .cloned()
        .map(CertificateDer::from)
        .collect();
    let certified = CertifiedKey::new(certificate_chain, signing_key);
    certified.keys_match().map_err(super::config_error)?;
    Ok(certified)
}

#[cfg(test)]
mod tests;
