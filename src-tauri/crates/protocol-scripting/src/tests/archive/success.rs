use std::io::Cursor;

use super::common::{
    build_zip, build_zip_with_comment, build_zip_with_timestamp, deflated_file, directory, file,
    valid_entries,
};
use crate::{PackageFilePath, ProtocolArchiveLimits, read_protocol_package_zip};

#[test]
fn valid_package_is_read_in_stable_path_order_without_directories() {
    let entries = [
        directory("scripts/"),
        file("scripts/protocol.rhai", b"fn frame() {}".to_vec()),
        file("manifest.toml", b"api = 1".to_vec()),
        file("empty.rhai", Vec::new()),
        file("document.toml", b"id = 'example'".to_vec()),
    ];
    let files = read_protocol_package_zip(
        Cursor::new(build_zip(&entries)),
        &ProtocolArchiveLimits::default(),
    )
    .unwrap();

    assert_eq!(files.len(), 4);
    assert!(!files.is_empty());
    assert_eq!(files.manifest(), b"api = 1");
    assert_eq!(
        files.get(&PackageFilePath::new("empty.rhai").unwrap()),
        Some(&[][..])
    );
    assert_eq!(
        files.total_bytes(),
        (b"fn frame() {}".len() + b"api = 1".len() + b"id = 'example'".len()) as u64
    );
    let paths: Vec<_> = files.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "document.toml",
            "empty.rhai",
            "manifest.toml",
            "scripts/protocol.rhai"
        ]
    );
}

#[test]
fn entry_order_and_stored_or_deflated_encoding_produce_the_same_files() {
    let stored = build_zip(&valid_entries());
    let timestamp = zip::DateTime::from_date_and_time(2024, 6, 7, 8, 9, 10).unwrap();
    let deflated_reversed = build_zip_with_timestamp(
        &[
            deflated_file("scripts/protocol.rhai", b"fn frame() {}".to_vec()),
            deflated_file("document.toml", b"id = 'example'".to_vec()),
            deflated_file("manifest.toml", b"api = 1".to_vec()),
        ],
        timestamp,
    );
    let limits = ProtocolArchiveLimits::default();
    let left = read_protocol_package_zip(Cursor::new(stored), &limits).unwrap();
    let right = read_protocol_package_zip(Cursor::new(deflated_reversed), &limits).unwrap();
    assert_eq!(left, right);
    assert_eq!(left.clone(), left);
}

#[test]
fn exact_entry_file_total_ratio_and_depth_limits_are_accepted() {
    let entries = [
        file("manifest.toml", b"1234".to_vec()),
        file("one/two/three.rhai", b"5678".to_vec()),
    ];
    let zip = build_zip(&entries);
    let limits = ProtocolArchiveLimits::new(zip.len() as u64, 2, 4, 8, 1, 3).unwrap();
    let files = read_protocol_package_zip(Cursor::new(zip), &limits).unwrap();
    assert_eq!(files.total_bytes(), 8);
}

#[test]
fn unicode_utf8_paths_are_preserved_without_platform_conversion() {
    let entries = [
        file("manifest.toml", b"api = 1".to_vec()),
        file("脚本/协议.rhai", b"fn frame() {}".to_vec()),
    ];
    let files = read_protocol_package_zip(
        Cursor::new(build_zip(&entries)),
        &ProtocolArchiveLimits::default(),
    )
    .unwrap();
    assert!(
        files
            .get(&PackageFilePath::new("脚本/协议.rhai").unwrap())
            .is_some()
    );
}

#[test]
fn zip_comments_including_eocd_signature_do_not_change_package_contents() {
    let comment = b"author note PK\x05\x06 is ordinary comment data";
    let files = read_protocol_package_zip(
        Cursor::new(build_zip_with_comment(&valid_entries(), comment)),
        &ProtocolArchiveLimits::default(),
    )
    .unwrap();
    assert_eq!(files.manifest(), b"api = 1");
}
