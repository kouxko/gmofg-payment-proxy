use std::{
    sync::{
        Arc, Barrier, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256, SanType};

use super::*;
use crate::MitmServerIdentity;

#[derive(Debug)]
struct TestAuthority {
    material: TestMaterial,
    calls: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
    fail: AtomicBool,
    gate: Gate,
}

#[derive(Debug)]
struct TestMaterial {
    certificate: Vec<u8>,
    private_key: Vec<u8>,
}

#[derive(Debug, Default)]
struct Gate {
    released: Mutex<bool>,
    ready: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut released = lock_recover(&self.released);
        while !*released {
            released = self
                .ready
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn release(&self) {
        *lock_recover(&self.released) = true;
        self.ready.notify_all();
    }
}

impl TestAuthority {
    fn new(fail: bool) -> Self {
        Self {
            material: test_material(),
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            fail: AtomicBool::new(fail),
            gate: Gate::default(),
        }
    }
}

impl MitmCertificateAuthority for TestAuthority {
    fn issue_server_identity(&self, _authority_host: &str) -> Result<MitmServerIdentity> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        self.gate.wait();
        self.active.fetch_sub(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(ProxyError::new(
                ErrorCode::CertificateInvalid,
                "test signing failure",
            ));
        }
        Ok(MitmServerIdentity {
            certificate_chain_der: vec![self.material.certificate.clone()],
            private_key_pkcs8_der: self.material.private_key.clone().into(),
        })
    }
}

#[test]
fn same_sni_waiters_share_one_successful_issuance() {
    const THREADS: usize = 8;
    let authority = Arc::new(TestAuthority::new(false));
    let resolver = resolver(authority.clone());
    let start = Arc::new(Barrier::new(THREADS));
    let handles = (0..THREADS)
        .map(|_| {
            let resolver = Arc::clone(&resolver);
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                resolver.resolve_identity("shared.example.test")
            })
        })
        .collect::<Vec<_>>();

    wait_for_calls(&authority, 1);
    thread::sleep(Duration::from_millis(30));
    assert_eq!(authority.calls.load(Ordering::SeqCst), 1);
    authority.gate.release();
    let identities = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect::<Vec<_>>();

    assert!(
        identities[1..]
            .iter()
            .all(|identity| Arc::ptr_eq(&identities[0], identity))
    );
}

#[test]
fn different_sni_issuances_run_concurrently() {
    let authority = Arc::new(TestAuthority::new(false));
    let resolver = resolver(authority.clone());
    let handles = ["first.example.test", "second.example.test"].map(|server_name| {
        let resolver = Arc::clone(&resolver);
        thread::spawn(move || resolver.resolve_identity(server_name))
    });

    wait_for_calls(&authority, 2);
    assert_eq!(authority.max_active.load(Ordering::SeqCst), 2);
    authority.gate.release();
    for handle in handles {
        handle.join().unwrap().unwrap();
    }
}

#[test]
fn same_sni_waiters_share_the_same_failure_without_reissuing() {
    const THREADS: usize = 8;
    let authority = Arc::new(TestAuthority::new(true));
    let resolver = resolver(authority.clone());
    let start = Arc::new(Barrier::new(THREADS));
    let handles = (0..THREADS)
        .map(|_| {
            let resolver = Arc::clone(&resolver);
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                resolver.resolve_identity("failed.example.test")
            })
        })
        .collect::<Vec<_>>();

    wait_for_calls(&authority, 1);
    thread::sleep(Duration::from_millis(30));
    authority.gate.release();
    let errors = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap_err())
        .collect::<Vec<_>>();

    assert_eq!(authority.calls.load(Ordering::SeqCst), 1);
    assert!(errors.iter().all(|error| {
        error.code == ErrorCode::CertificateInvalid.as_str()
            && error.message == "test signing failure"
    }));
}

#[test]
fn stale_flight_owner_cannot_remove_a_newer_flight() {
    let authority = Arc::new(TestAuthority::new(false));
    let resolver = resolver(authority);
    let stale = Arc::new(IdentityFlight::default());
    let current = Arc::new(IdentityFlight::default());
    lock_recover(&resolver.state)
        .in_flight
        .insert("retry.example.test".into(), Arc::clone(&current));

    resolver.finish_flight("retry.example.test", &stale, None);

    let flight_state = lock_recover(&resolver.state);
    assert!(Arc::ptr_eq(
        flight_state.in_flight.get("retry.example.test").unwrap(),
        &current
    ));
}

#[test]
fn in_flight_limit_rejects_excess_work_and_recovers_capacity() {
    let authority = Arc::new(TestAuthority::new(false));
    let resolver = resolver(authority.clone());
    let handles = (0..MAX_DYNAMIC_IDENTITY_FLIGHTS)
        .map(|index| {
            let resolver = Arc::clone(&resolver);
            thread::spawn(move || resolver.resolve_identity(&format!("host-{index}.example.test")))
        })
        .collect::<Vec<_>>();

    wait_for_calls(&authority, MAX_DYNAMIC_IDENTITY_FLIGHTS);
    let error = resolver
        .resolve_identity("overflow.example.test")
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::OperationInProgress.as_str());
    authority.gate.release();
    for handle in handles {
        handle.join().unwrap().unwrap();
    }

    resolver.resolve_identity("recovered.example.test").unwrap();
    assert_eq!(
        authority.calls.load(Ordering::SeqCst),
        MAX_DYNAMIC_IDENTITY_FLIGHTS + 1
    );
}

#[test]
fn cache_evicts_least_recently_used_entry() {
    let mut cache = DynamicServerIdentityCache::default();
    for index in 0..MAX_DYNAMIC_SERVER_IDENTITIES {
        cache.insert(format!("host-{index}.example.test"), Arc::new(index));
    }
    assert!(cache.get("host-0.example.test").is_some());

    cache.insert("new.example.test".into(), Arc::new(999));

    assert!(cache.entries.contains_key("host-0.example.test"));
    assert!(!cache.entries.contains_key("host-1.example.test"));
    assert_eq!(cache.entries.len(), MAX_DYNAMIC_SERVER_IDENTITIES);
}

fn resolver(authority: Arc<TestAuthority>) -> Arc<DynamicServerIdentityResolver> {
    let fallback_material = test_material();
    let fallback = certified_key_from_parts(
        &[fallback_material.certificate],
        fallback_material.private_key,
    )
    .unwrap();
    Arc::new(DynamicServerIdentityResolver::new(
        authority,
        vec!["*.example.test".into()],
        Arc::new(fallback),
    ))
}

fn test_material() -> TestMaterial {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    params.subject_alt_names = vec![SanType::DnsName("example.test".try_into().unwrap())];
    let certificate = params.self_signed(&key).unwrap();
    TestMaterial {
        certificate: certificate.der().to_vec(),
        private_key: key.serialize_der(),
    }
}

fn wait_for_calls(authority: &TestAuthority, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while authority.calls.load(Ordering::SeqCst) < expected {
        assert!(Instant::now() < deadline, "timed out waiting for issuance");
        thread::yield_now();
    }
}
