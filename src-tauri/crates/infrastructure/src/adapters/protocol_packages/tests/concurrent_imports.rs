use super::*;

#[test]
fn concurrent_different_content_never_overwrites_the_winning_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    SqliteStore::open(&path).unwrap();
    let archives = [
        package_zip(MANIFEST, SCRIPT),
        package_zip(MANIFEST, &SCRIPT.replace("origin }", "blob() }")),
    ];
    let barrier = Arc::new(Barrier::new(2));
    let stores = [
        Arc::new(SqliteStore::open(&path).unwrap()),
        Arc::new(SqliteStore::open(&path).unwrap()),
    ];
    let threads = archives
        .into_iter()
        .zip(stores)
        .map(|(zip, store)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let repository = ProtocolPackageRepositoryAdapter::with_default_limits(store);
                barrier.wait();
                repository.install_zip(&zip)
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(error) if error.code() == ProtocolPackageStorageErrorCode::IdentityConflict
            ))
            .count(),
        1
    );
    let reopened = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::open(&path).unwrap(),
    ));
    assert_eq!(reopened.list().unwrap().len(), 1);
    assert_eq!(
        reopened.recover_cache().unwrap().loaded,
        vec![package("1.0.0")]
    );
}

#[test]
fn concurrent_mixed_kind_versions_commit_only_one_kind_for_the_package_id() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    SqliteStore::open(&path).unwrap();
    let http_v2 = MANIFEST
        .replace("version = \"1.0.0\"", "version = \"2.0.0\"")
        .replace("frame = \"frame\"\n", "");
    let archives = [package_zip(MANIFEST, SCRIPT), package_zip(&http_v2, SCRIPT)];
    let barrier = Arc::new(Barrier::new(2));
    let stores = [
        Arc::new(SqliteStore::open(&path).unwrap()),
        Arc::new(SqliteStore::open(&path).unwrap()),
    ];
    let threads = archives
        .into_iter()
        .zip(stores)
        .map(|(zip, store)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let repository = ProtocolPackageRepositoryAdapter::with_default_limits(store);
                barrier.wait();
                repository.install_zip(&zip)
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(error) if error.code() == ProtocolPackageStorageErrorCode::IdentityConflict
            ))
            .count(),
        1
    );
    let reopened = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::open(&path).unwrap(),
    ));
    let installed = reopened.list().unwrap();
    assert_eq!(installed.len(), 1);
    assert!(installed.iter().all(|item| item.kind == installed[0].kind));
}
