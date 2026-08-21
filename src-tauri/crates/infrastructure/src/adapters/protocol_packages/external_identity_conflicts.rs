//! 内部安装入口的跨来源精确身份冲突回归。

use super::*;

#[test]
fn external_exact_identity_blocks_internal_install_without_partial_write() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .execute_test_batch(
            "INSERT INTO external_protocol_packages(
                package_id, version, registration_json, registration_fingerprint,
                enabled, first_connected_at, last_connected_at
             ) VALUES (
                'example-protocol', '1.0.0', '{}', zeroblob(32), 0,
                '2026-08-20T00:00:00Z', '2026-08-20T00:00:00Z'
             );",
        )
        .unwrap();
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));

    let error = repository
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageStorageErrorCode::IdentityConflict
    );
    assert_eq!(store.protocol_package_row_counts_for_test(), (0, 0));
}
