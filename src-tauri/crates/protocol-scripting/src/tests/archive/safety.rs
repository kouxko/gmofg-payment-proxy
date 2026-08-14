use std::io::Cursor;

use super::common::{
    build_zip, corrupt_first_file_data, deflated_file, directory, file,
    patch_central_uncompressed_size, patch_compression, patch_encrypted, patch_entry_name,
    patch_eocd_disk, patch_raw_name_non_utf8, patch_second_entry_to_overlap_first, patch_unix_mode,
    valid_entries,
};
use crate::{ProtocolArchiveErrorCode, ProtocolArchiveLimits, read_protocol_package_zip};

fn read_error(bytes: Vec<u8>) -> ProtocolArchiveErrorCode {
    read_protocol_package_zip(Cursor::new(bytes), &ProtocolArchiveLimits::default())
        .unwrap_err()
        .code()
}

#[test]
fn empty_corrupt_and_crc_damaged_archives_fail_without_panicking() {
    assert_eq!(
        read_error(Vec::new()),
        ProtocolArchiveErrorCode::EmptyArchive
    );
    assert_eq!(
        read_error(build_zip(&[])),
        ProtocolArchiveErrorCode::EmptyArchive
    );

    let mut truncated = build_zip(&valid_entries());
    truncated.truncate(truncated.len() - 12);
    assert_eq!(read_error(truncated), ProtocolArchiveErrorCode::InvalidZip);

    let mut corrupted = build_zip(&valid_entries());
    corrupt_first_file_data(&mut corrupted);
    assert_eq!(read_error(corrupted), ProtocolArchiveErrorCode::InvalidZip);

    let mut multi_disk = build_zip(&valid_entries());
    patch_eocd_disk(&mut multi_disk, 1);
    assert_eq!(read_error(multi_disk), ProtocolArchiveErrorCode::InvalidZip);

    for bytes in [vec![0], b"not a zip".to_vec(), vec![0x50, 0x4b, 0x03, 0x04]] {
        assert!(
            read_protocol_package_zip(Cursor::new(bytes), &ProtocolArchiveLimits::default())
                .is_err()
        );
    }
}

#[test]
fn inconsistent_directory_and_local_file_metadata_are_rejected() {
    let directory_with_data = build_zip(&[
        file("manifest.toml", b"api = 1".to_vec()),
        file("folder/", b"not a directory".to_vec()),
    ]);
    assert_eq!(
        read_error(directory_with_data),
        ProtocolArchiveErrorCode::InvalidZip
    );

    let mut declared_size_mismatch = build_zip(&valid_entries());
    patch_central_uncompressed_size(&mut declared_size_mismatch, b"manifest.toml", 6);
    assert_eq!(
        read_error(declared_size_mismatch),
        ProtocolArchiveErrorCode::InvalidZip
    );

    let mut actual_over_limit = build_zip(&valid_entries());
    patch_central_uncompressed_size(&mut actual_over_limit, b"manifest.toml", 6);
    let limits = ProtocolArchiveLimits::new(10_000, 10, 6, 100, 10, 4).unwrap();
    assert_eq!(
        read_protocol_package_zip(Cursor::new(actual_over_limit), &limits)
            .unwrap_err()
            .code(),
        ProtocolArchiveErrorCode::FileTooLarge
    );
}

#[test]
fn root_manifest_is_required_without_unwrapping_outer_directories() {
    let wrapped = build_zip(&[
        directory("package/"),
        file("package/manifest.toml", b"api = 1".to_vec()),
    ]);
    assert_eq!(
        read_error(wrapped),
        ProtocolArchiveErrorCode::ManifestMissing
    );

    let directory_only = build_zip(&[directory("manifest.toml/")]);
    assert_eq!(
        read_error(directory_only),
        ProtocolArchiveErrorCode::ManifestMissing
    );
}

#[test]
fn unsafe_cross_platform_path_matrix_is_rejected_before_values_are_exposed() {
    for path in [
        "/manifest.toml",
        "../manifest.toml",
        "scripts/../manifest.toml",
        "./manifest.toml",
        "scripts/./main.rhai",
        "scripts//main.rhai",
        "C:/manifest.toml",
        "scripts\\main.rhai",
        "\\\\server\\share\\main.rhai",
        "scripts/evil\0name.rhai",
    ] {
        let zip = build_zip(&[
            file(path, b"bad".to_vec()),
            file("manifest.toml", b"api = 1".to_vec()),
        ]);
        let error = read_protocol_package_zip(Cursor::new(zip), &ProtocolArchiveLimits::default())
            .unwrap_err();
        assert_eq!(
            error.code(),
            ProtocolArchiveErrorCode::InvalidPath,
            "{path}"
        );
        assert_eq!(error.path(), None, "unsafe path must never be echoed");
    }

    let too_long = "a".repeat(crate::MAX_PACKAGE_FILE_PATH_BYTES + 1);
    let error = read_protocol_package_zip(
        Cursor::new(build_zip(&[
            file(&too_long, b"bad".to_vec()),
            file("manifest.toml", b"api = 1".to_vec()),
        ])),
        &ProtocolArchiveLimits::default(),
    )
    .unwrap_err();
    assert_eq!(error.code(), ProtocolArchiveErrorCode::InvalidPath);
    assert_eq!(error.path(), None);
}

#[test]
fn non_utf8_raw_names_are_rejected_even_if_zip_can_decode_legacy_names() {
    let mut zip = build_zip(&valid_entries());
    patch_raw_name_non_utf8(&mut zip, b"manifest.toml");
    let error =
        read_protocol_package_zip(Cursor::new(zip), &ProtocolArchiveLimits::default()).unwrap_err();
    assert_eq!(error.code(), ProtocolArchiveErrorCode::NonUtf8Path);
    assert_eq!(error.entry_index(), Some(0));
    assert_eq!(error.path(), None);
}

#[test]
fn exact_duplicates_case_conflicts_and_file_directory_conflicts_are_rejected() {
    let mut duplicate = build_zip(&[
        file("manifest.toml", b"first".to_vec()),
        file("secondxx.toml", b"second".to_vec()),
    ]);
    patch_entry_name(&mut duplicate, b"secondxx.toml", b"manifest.toml");
    assert_eq!(
        read_error(duplicate),
        ProtocolArchiveErrorCode::DuplicatePath
    );

    let case_conflict = build_zip(&[
        file("manifest.toml", b"api = 1".to_vec()),
        file("Scripts/Main.rhai", b"one".to_vec()),
        file("scripts/main.rhai", b"two".to_vec()),
    ]);
    assert_eq!(
        read_error(case_conflict),
        ProtocolArchiveErrorCode::CaseConflict
    );

    for entries in [
        vec![
            file("manifest.toml", b"api = 1".to_vec()),
            file("scripts", b"file".to_vec()),
            file("scripts/main.rhai", b"child".to_vec()),
        ],
        vec![
            file("manifest.toml", b"api = 1".to_vec()),
            file("scripts/main.rhai", b"child".to_vec()),
            file("scripts", b"file".to_vec()),
        ],
    ] {
        assert_eq!(
            read_error(build_zip(&entries)),
            ProtocolArchiveErrorCode::PathTypeConflict
        );
    }
}

#[test]
fn symlinks_special_files_encryption_and_unknown_compression_are_rejected() {
    let mut symlink = build_zip(&valid_entries());
    patch_unix_mode(&mut symlink, b"manifest.toml", 0o120_777);
    assert_eq!(
        read_error(symlink),
        ProtocolArchiveErrorCode::SymlinkForbidden
    );

    let mut fifo = build_zip(&valid_entries());
    patch_unix_mode(&mut fifo, b"manifest.toml", 0o010_644);
    assert_eq!(
        read_error(fifo),
        ProtocolArchiveErrorCode::UnsupportedEntryType
    );

    let mut encrypted = build_zip(&valid_entries());
    patch_encrypted(&mut encrypted, b"manifest.toml");
    assert_eq!(
        read_error(encrypted),
        ProtocolArchiveErrorCode::EncryptedEntry
    );

    let mut compression = build_zip(&valid_entries());
    // BZip2 是 ZIP 标准方法，但本 crate 刻意没有启用对应 feature。
    patch_compression(&mut compression, b"manifest.toml", 12);
    assert_eq!(
        read_error(compression),
        ProtocolArchiveErrorCode::UnsupportedCompression
    );
}

#[test]
fn overlapping_compressed_ranges_are_rejected() {
    let mut zip = build_zip(&valid_entries());
    patch_second_entry_to_overlap_first(&mut zip);
    assert_eq!(
        read_error(zip),
        ProtocolArchiveErrorCode::OverlappingEntries
    );
}

#[test]
fn entry_count_file_total_archive_ratio_and_depth_limits_fail_closed() {
    let too_many = build_zip(&[
        file("manifest.toml", b"a".to_vec()),
        file("second.rhai", b"b".to_vec()),
    ]);
    let limits = ProtocolArchiveLimits::new(10_000, 1, 10, 10, 10, 4).unwrap();
    assert_eq!(
        read_protocol_package_zip(Cursor::new(too_many), &limits)
            .unwrap_err()
            .code(),
        ProtocolArchiveErrorCode::TooManyEntries
    );

    let file_too_large = build_zip(&[file("manifest.toml", b"12345".to_vec())]);
    let limits = ProtocolArchiveLimits::new(10_000, 2, 4, 8, 10, 4).unwrap();
    assert_eq!(
        read_protocol_package_zip(Cursor::new(file_too_large), &limits)
            .unwrap_err()
            .code(),
        ProtocolArchiveErrorCode::FileTooLarge
    );

    let total_too_large = build_zip(&[
        file("manifest.toml", b"1234".to_vec()),
        file("second.rhai", b"5678".to_vec()),
    ]);
    let limits = ProtocolArchiveLimits::new(10_000, 2, 4, 7, 10, 4).unwrap();
    assert_eq!(
        read_protocol_package_zip(Cursor::new(total_too_large), &limits)
            .unwrap_err()
            .code(),
        ProtocolArchiveErrorCode::TotalTooLarge
    );

    let bomb = build_zip(&[deflated_file("manifest.toml", vec![0; 16 * 1024])]);
    let limits = ProtocolArchiveLimits::new(100_000, 2, 20_000, 20_000, 2, 4).unwrap();
    assert_eq!(
        read_protocol_package_zip(Cursor::new(bomb), &limits)
            .unwrap_err()
            .code(),
        ProtocolArchiveErrorCode::CompressionRatioExceeded
    );

    let deep = build_zip(&[
        file("manifest.toml", b"ok".to_vec()),
        file("one/two/three/four.rhai", b"deep".to_vec()),
    ]);
    let limits = ProtocolArchiveLimits::new(10_000, 2, 10, 20, 10, 3).unwrap();
    assert_eq!(
        read_protocol_package_zip(Cursor::new(deep), &limits)
            .unwrap_err()
            .code(),
        ProtocolArchiveErrorCode::PathTooDeep
    );

    let archive = build_zip(&valid_entries());
    let limits = ProtocolArchiveLimits::new(archive.len() as u64 - 1, 10, 100, 300, 10, 4).unwrap();
    assert_eq!(
        read_protocol_package_zip(Cursor::new(archive), &limits)
            .unwrap_err()
            .code(),
        ProtocolArchiveErrorCode::ArchiveTooLarge
    );
}
