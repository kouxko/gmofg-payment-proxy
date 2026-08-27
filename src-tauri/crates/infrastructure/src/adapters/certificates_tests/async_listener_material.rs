#[tokio::test(flavor = "current_thread")]
async fn listener_identity_and_mitm_freeze_wait_asynchronously_and_cancel_safely() {
    use std::{future::Future, pin::Pin, sync::mpsc, task::Poll};

    use tokio::sync::oneshot;

    let adapter = CertificateServiceAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog {
            open: ParkingMutex::new(VecDeque::new()),
        }),
        test_profile(),
    );
    let executor = adapter.executor.clone();
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let blocker = tokio::spawn(async move {
        executor
            .execute(move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, InfrastructureError>(())
            })
            .await
    });
    entered_rx.await.unwrap();

    let mut identity = Box::pin(adapter.load_installation_server_identity());
    let polled = std::future::poll_fn(|context| {
        Poll::Ready(Pin::new(&mut identity).poll(context))
    })
    .await;
    assert!(matches!(polled, Poll::Pending));
    let (progress_tx, progress_rx) = oneshot::channel();
    tokio::spawn(async move { progress_tx.send(()).unwrap() });
    progress_rx.await.unwrap();
    drop(identity);

    let mut authority = Box::pin(adapter.freeze_installation_tls_material());
    let polled = std::future::poll_fn(|context| {
        Poll::Ready(Pin::new(&mut authority).poll(context))
    })
    .await;
    assert!(matches!(polled, Poll::Pending));
    drop(authority);

    release_tx.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    let error = adapter
        .load_installation_server_identity()
        .await
        .expect_err("missing identity remains an explicit failure");
    assert_eq!(error.view_model.code, "CERTIFICATE_NOT_READY");
}

#[derive(Debug)]
struct BlockingUnprotectProtector {
    entered: ParkingMutex<Option<std::sync::mpsc::Sender<()>>>,
    release: ParkingMutex<std::sync::mpsc::Receiver<()>>,
}

impl SecretProtector for BlockingUnprotectProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        XorProtector.protect(plaintext)
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        if let Some(entered) = self.entered.lock().take() {
            entered.send(()).unwrap();
            self.release.lock().recv().unwrap();
        }
        XorProtector.unprotect(ciphertext)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn installation_tls_snapshot_cannot_mix_fallback_and_dynamic_ca_generations() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    let seed = CertificateServiceAdapter::new(
        store.clone(),
        Arc::new(XorProtector),
        Arc::new(QueueDialog {
            open: ParkingMutex::new(VecDeque::new()),
        }),
        test_profile(),
    );
    let service = CertificateService;
    let root_a = service.generate_root_ca("Root A").expect("root A");
    let leaf_a = service
        .generate_leaf(
            &root_a.certificate_der,
            &root_a.private_key_pkcs8_der,
            &leaf_request(&["fallback-a.example.test".into()]).expect("leaf A request"),
        )
        .expect("leaf A");
    let root_b = service.generate_root_ca("Root B").expect("root B");
    let leaf_b = service
        .generate_leaf(
            &root_b.certificate_der,
            &root_b.private_key_pkcs8_der,
            &leaf_request(&["fallback-b.example.test".into()]).expect("leaf B request"),
        )
        .expect("leaf B");
    let mut initial = seed.load_snapshot(&MATERIAL_KINDS).expect("initial snapshot");
    initial.materials.insert(ROOT.into(), from_bundle(1, &root_a));
    initial.materials.insert(LEAF.into(), from_bundle(1, &leaf_a));
    seed.commit_snapshot(initial).expect("seed A");

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let adapter = Arc::new(CertificateServiceAdapter::new(
        store,
        Arc::new(BlockingUnprotectProtector {
            entered: ParkingMutex::new(Some(entered_tx)),
            release: ParkingMutex::new(release_rx),
        }),
        Arc::new(QueueDialog {
            open: ParkingMutex::new(VecDeque::new()),
        }),
        test_profile(),
    ));
    let freezing = {
        let adapter = adapter.clone();
        tokio::spawn(async move { adapter.freeze_installation_tls_material().await })
    };
    entered_rx.recv().expect("A snapshot reached decode barrier");
    let mut replacement = seed.load_snapshot(&MATERIAL_KINDS).expect("A snapshot");
    replacement
        .materials
        .insert(ROOT.into(), from_bundle(2, &root_b));
    replacement
        .materials
        .insert(LEAF.into(), from_bundle(2, &leaf_b));
    seed.commit_snapshot(replacement).expect("switch to B");
    release_tx.send(()).unwrap();

    let epoch_a = freezing.await.unwrap().expect("frozen A generation");
    assert_eq!(
        epoch_a.server_identity.certificate_chain_der,
        vec![leaf_a.certificate_der.clone()]
    );
    let issued_a = epoch_a
        .dynamic_authority
        .issue_server_identity("api.example.test")
        .expect("A dynamic identity");
    service
        .validate_leaf(
            &root_a.certificate_der,
            &issued_a.certificate_chain_der[0],
            &issued_a.private_key_pkcs8_der,
            &["api.example.test".into()],
        )
        .expect("dynamic identity remains signed by A");

    let epoch_b = adapter
        .freeze_installation_tls_material()
        .await
        .expect("restart freezes B");
    assert_eq!(
        epoch_b.server_identity.certificate_chain_der,
        vec![leaf_b.certificate_der.clone()]
    );
}
