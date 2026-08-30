use std::io::{Cursor, Write};

use intercept_proxy_domain::ErrorCode;
use intercept_proxy_package_runtime::{
    PackageArchive, PackageArchiveResourceLimits, read_package_zip,
};
use zip::{ZipWriter, write::SimpleFileOptions};

const MANIFEST: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);

struct Limits {
    archive: u64,
    entries: usize,
    file: u64,
    total: u64,
    ratio: u64,
    depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            archive: 8 * 1024 * 1024,
            entries: 64,
            file: 1024 * 1024,
            total: 4 * 1024 * 1024,
            ratio: 100,
            depth: 8,
        }
    }
}
impl PackageArchiveResourceLimits for Limits {
    fn max_archive_bytes(&self) -> u64 {
        self.archive
    }
    fn max_entries(&self) -> usize {
        self.entries
    }
    fn max_file_bytes(&self) -> u64 {
        self.file
    }
    fn max_total_bytes(&self) -> u64 {
        self.total
    }
    fn max_compression_ratio(&self) -> u64 {
        self.ratio
    }
    fn max_path_depth(&self) -> usize {
        self.depth
    }
}

fn package_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut output);
        for (path, bytes) in entries {
            writer
                .start_file(*path, SimpleFileOptions::default())
                .expect("start entry");
            writer.write_all(bytes).expect("write entry");
        }
        writer.finish().expect("finish archive");
    }
    output.into_inner()
}

fn compressed_package_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut output);
        for (path, bytes) in entries {
            writer
                .start_file(
                    *path,
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    output.into_inner()
}

fn valid_zip() -> Vec<u8> {
    package_zip(&[
        ("manifest.json", MANIFEST.as_bytes()),
        ("protocol.js", b"export function upstreamDecode() {}"),
        ("display.js", b"export function upstreamDisplay() {}"),
        ("lib/value.js", b"export const value = 1;"),
    ])
}

fn patch_central_uncompressed_size(bytes: &mut [u8], file_name: &str, size: u32) {
    const CENTRAL_HEADER: &[u8; 4] = b"PK\x01\x02";
    let name = file_name.as_bytes();
    let offset = bytes
        .windows(CENTRAL_HEADER.len())
        .position(|window| window == CENTRAL_HEADER)
        .and_then(|first| {
            bytes[first..]
                .windows(CENTRAL_HEADER.len())
                .enumerate()
                .find_map(|(relative, window)| {
                    if window != CENTRAL_HEADER {
                        return None;
                    }
                    let header = first + relative;
                    let name_start = header + 46;
                    bytes
                        .get(name_start..name_start + name.len())
                        .filter(|candidate| *candidate == name)
                        .map(|_| header)
                })
        })
        .expect("central directory entry");
    bytes[offset + 24..offset + 28].copy_from_slice(&size.to_le_bytes());
}

#[test]
fn root_manifest_protocol_display_and_relative_js_modules_are_accepted() {
    let archive: PackageArchive =
        read_package_zip(Cursor::new(valid_zip()), &Limits::default()).expect("valid ZIP");
    assert_eq!(
        archive.manifest().package().identity().id.as_str(),
        "com.example.payment"
    );
    assert_eq!(
        archive.file("protocol.js"),
        Some(b"export function upstreamDecode() {}".as_slice())
    );
    assert_eq!(
        archive.file("lib/value.js"),
        Some(b"export const value = 1;".as_slice())
    );
}

#[test]
fn missing_fixed_root_file_is_protocol_package_invalid() {
    for missing in ["manifest.json", "protocol.js", "display.js"] {
        let entries = [
            ("manifest.json", MANIFEST.as_bytes()),
            ("protocol.js", b"export {}".as_slice()),
            ("display.js", b"export {}".as_slice()),
        ];
        let filtered = entries
            .into_iter()
            .filter(|(path, _)| *path != missing)
            .collect::<Vec<_>>();
        let error = read_package_zip(Cursor::new(package_zip(&filtered)), &Limits::default())
            .expect_err("missing root must fail");
        assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid);
    }
}

#[test]
fn directory_wrappers_typescript_and_non_js_payloads_are_rejected() {
    for path in ["package/manifest.json", "protocol.ts", "README.md"] {
        let mut entries = vec![
            ("manifest.json", MANIFEST.as_bytes()),
            ("protocol.js", b"export {}".as_slice()),
            ("display.js", b"export {}".as_slice()),
        ];
        entries.push((path, b"invalid"));
        let error = read_package_zip(Cursor::new(package_zip(&entries)), &Limits::default())
            .expect_err("invalid layout");
        assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid, "{path}");
    }
}

#[test]
fn manifest_json_uses_the_shared_strict_contract() {
    let invalid = MANIFEST.replacen("\"api\": 1", "\"api\": 1, \"hooks\": {}", 1);
    let bytes = package_zip(&[
        ("manifest.json", invalid.as_bytes()),
        ("protocol.js", b"export {}"),
        ("display.js", b"export {}"),
    ]);
    let error = read_package_zip(Cursor::new(bytes), &Limits::default())
        .expect_err("unknown field must fail");
    assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid);
}

#[test]
fn archive_entry_file_total_ratio_and_depth_limits_fail_closed() {
    let bytes = valid_zip();
    let cases = [
        Limits {
            archive: bytes.len() as u64 - 1,
            ..Limits::default()
        },
        Limits {
            entries: 3,
            ..Limits::default()
        },
        Limits {
            file: 8,
            ..Limits::default()
        },
        Limits {
            total: 32,
            ..Limits::default()
        },
        Limits {
            depth: 1,
            ..Limits::default()
        },
    ];
    for limits in cases {
        let error = read_package_zip(Cursor::new(&bytes), &limits).expect_err("limit must fail");
        assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid);
    }
    let repeated = vec![b'a'; 4096];
    let bomb = compressed_package_zip(&[
        ("manifest.json", MANIFEST.as_bytes()),
        ("protocol.js", &repeated),
        ("display.js", b"export {}"),
    ]);
    let error = read_package_zip(
        Cursor::new(bomb),
        &Limits {
            ratio: 1,
            ..Limits::default()
        },
    )
    .expect_err("compression ratio must fail");
    assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid);
}

#[test]
fn declared_and_actual_entry_sizes_must_match() {
    let mut bytes = valid_zip();
    patch_central_uncompressed_size(&mut bytes, "protocol.js", 1);
    let error = read_package_zip(Cursor::new(bytes), &Limits::default())
        .expect_err("forged declared size must fail closed");
    assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid);
}
