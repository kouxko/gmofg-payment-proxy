use crate::{
    PackageFilePath, ProtocolArchiveErrorCode, ProtocolArchiveLimits,
    restore_protocol_package_files,
};

fn file(path: &str, bytes: impl Into<Vec<u8>>) -> (String, Vec<u8>) {
    (path.to_owned(), bytes.into())
}

fn restore_error(
    files: Vec<(String, Vec<u8>)>,
    limits: &ProtocolArchiveLimits,
) -> ProtocolArchiveErrorCode {
    restore_protocol_package_files(files, limits)
        .unwrap_err()
        .code()
}

#[test]
fn persisted_rows_restore_to_a_stable_validated_file_collection() {
    let restored = restore_protocol_package_files(
        vec![
            file("scripts/protocol.rhai", b"fn frame() {}".to_vec()),
            file("manifest.toml", b"api = 1".to_vec()),
            file("empty.rhai", Vec::new()),
        ],
        &ProtocolArchiveLimits::default(),
    )
    .unwrap();

    assert_eq!(restored.len(), 3);
    assert_eq!(restored.total_bytes(), 20);
    assert_eq!(restored.manifest(), b"api = 1");
    assert_eq!(
        restored.get(&PackageFilePath::new("empty.rhai").unwrap()),
        Some(&[][..])
    );
    let paths: Vec<_> = restored.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        ["empty.rhai", "manifest.toml", "scripts/protocol.rhai"]
    );
}

#[test]
fn empty_or_manifest_less_persisted_rows_are_rejected() {
    assert_eq!(
        restore_error(Vec::new(), &ProtocolArchiveLimits::default()),
        ProtocolArchiveErrorCode::EmptyArchive
    );
    assert_eq!(
        restore_error(
            vec![file("scripts/protocol.rhai", b"fn frame() {}".to_vec())],
            &ProtocolArchiveLimits::default()
        ),
        ProtocolArchiveErrorCode::ManifestMissing
    );
}

#[test]
fn persisted_paths_are_revalidated_before_being_exposed() {
    for path in [
        "/manifest.toml",
        "../manifest.toml",
        "scripts/../manifest.toml",
        "./manifest.toml",
        "scripts//main.rhai",
        "C:/manifest.toml",
        "scripts\\main.rhai",
        "scripts/evil\0name.rhai",
    ] {
        let error = restore_protocol_package_files(
            vec![file(path, b"bad".to_vec())],
            &ProtocolArchiveLimits::default(),
        )
        .unwrap_err();
        assert_eq!(
            error.code(),
            ProtocolArchiveErrorCode::InvalidPath,
            "{path}"
        );
        assert_eq!(error.entry_index(), Some(0));
        assert_eq!(error.path(), None, "不安全原始路径不能出现在错误中");
    }
}

#[test]
fn exact_duplicate_case_conflict_and_file_parent_conflict_are_rejected() {
    let limits = ProtocolArchiveLimits::default();
    assert_eq!(
        restore_error(
            vec![
                file("manifest.toml", b"first".to_vec()),
                file("manifest.toml", b"second".to_vec()),
            ],
            &limits,
        ),
        ProtocolArchiveErrorCode::DuplicatePath
    );
    assert_eq!(
        restore_error(
            vec![
                file("manifest.toml", b"ok".to_vec()),
                file("Scripts/Main.rhai", b"one".to_vec()),
                file("scripts/main.rhai", b"two".to_vec()),
            ],
            &limits,
        ),
        ProtocolArchiveErrorCode::CaseConflict
    );

    for rows in [
        vec![
            file("manifest.toml", b"ok".to_vec()),
            file("scripts", b"file".to_vec()),
            file("scripts/main.rhai", b"child".to_vec()),
        ],
        vec![
            file("manifest.toml", b"ok".to_vec()),
            file("scripts/main.rhai", b"child".to_vec()),
            file("scripts", b"file".to_vec()),
        ],
    ] {
        assert_eq!(
            restore_error(rows, &limits),
            ProtocolArchiveErrorCode::PathTypeConflict
        );
    }
}

#[test]
fn count_file_total_and_depth_limits_are_reapplied_to_actual_rows() {
    let exact = ProtocolArchiveLimits::new(1, 2, 4, 8, 1, 3).unwrap();
    let restored = restore_protocol_package_files(
        vec![
            file("manifest.toml", b"1234".to_vec()),
            file("one/two/file.rhai", b"5678".to_vec()),
        ],
        &exact,
    )
    .unwrap();
    assert_eq!(restored.total_bytes(), 8);

    let too_many = ProtocolArchiveLimits::new(1, 1, 8, 16, 1, 4).unwrap();
    assert_eq!(
        restore_error(
            vec![
                file("manifest.toml", b"ok".to_vec()),
                file("second.rhai", b"x".to_vec()),
            ],
            &too_many,
        ),
        ProtocolArchiveErrorCode::TooManyEntries
    );

    let small_file = ProtocolArchiveLimits::new(1, 2, 3, 8, 1, 4).unwrap();
    assert_eq!(
        restore_error(vec![file("manifest.toml", b"1234".to_vec())], &small_file),
        ProtocolArchiveErrorCode::FileTooLarge
    );

    let small_total = ProtocolArchiveLimits::new(1, 2, 4, 7, 1, 4).unwrap();
    assert_eq!(
        restore_error(
            vec![
                file("manifest.toml", b"1234".to_vec()),
                file("second.rhai", b"5678".to_vec()),
            ],
            &small_total,
        ),
        ProtocolArchiveErrorCode::TotalTooLarge
    );

    let shallow = ProtocolArchiveLimits::new(1, 2, 8, 16, 1, 2).unwrap();
    let error = restore_protocol_package_files(
        vec![
            file("manifest.toml", b"ok".to_vec()),
            file("one/two/three.rhai", b"x".to_vec()),
        ],
        &shallow,
    )
    .unwrap_err();
    assert_eq!(error.code(), ProtocolArchiveErrorCode::PathTooDeep);
    assert_eq!(error.entry_index(), Some(1));
    assert_eq!(
        error.path().map(PackageFilePath::as_str),
        Some("one/two/three.rhai")
    );
}
