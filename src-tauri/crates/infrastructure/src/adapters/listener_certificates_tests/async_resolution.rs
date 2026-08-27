use std::{future::Future, pin::Pin, sync::mpsc, task::Poll};

use intercept_proxy_application::{CertificateReference, CertificateReferenceKind};
use tokio::sync::oneshot;

use super::*;

#[tokio::test(flavor = "current_thread")]
async fn managed_certificate_resolution_waits_asynchronously_and_cancel_does_not_strand_gate() {
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::new()))),
    );
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "missing".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: format!("{REFERENCE_PREFIX}missing"),
    };
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

    let mut resolution = Box::pin(adapter.resolve_identity(&reference));
    let polled =
        std::future::poll_fn(|context| Poll::Ready(Pin::new(&mut resolution).poll(context))).await;
    assert!(matches!(polled, Poll::Pending));
    let (progress_tx, progress_rx) = oneshot::channel();
    tokio::spawn(async move { progress_tx.send(()).unwrap() });
    progress_rx.await.unwrap();
    drop(resolution);

    release_tx.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    let error = adapter
        .resolve_identity(&reference)
        .await
        .expect("managed reference recognized")
        .expect_err("missing material remains explicit");
    assert_eq!(error.view_model.code, "CERTIFICATE_NOT_READY");
}
