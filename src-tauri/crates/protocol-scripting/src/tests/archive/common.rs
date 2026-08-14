use std::io::{Cursor, Write};

use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

#[derive(Clone, Debug)]
pub(super) enum TestEntry {
    File {
        name: String,
        bytes: Vec<u8>,
        compression: CompressionMethod,
    },
    Directory(String),
}

pub(super) fn file(name: &str, bytes: impl Into<Vec<u8>>) -> TestEntry {
    TestEntry::File {
        name: name.to_owned(),
        bytes: bytes.into(),
        compression: CompressionMethod::Stored,
    }
}

pub(super) fn deflated_file(name: &str, bytes: impl Into<Vec<u8>>) -> TestEntry {
    TestEntry::File {
        name: name.to_owned(),
        bytes: bytes.into(),
        compression: CompressionMethod::Deflated,
    }
}

pub(super) fn directory(name: &str) -> TestEntry {
    TestEntry::Directory(name.to_owned())
}

pub(super) fn build_zip(entries: &[TestEntry]) -> Vec<u8> {
    build_zip_with_metadata(entries, &[], None)
}

pub(super) fn build_zip_with_comment(entries: &[TestEntry], comment: &[u8]) -> Vec<u8> {
    build_zip_with_metadata(entries, comment, None)
}

pub(super) fn build_zip_with_timestamp(entries: &[TestEntry], timestamp: DateTime) -> Vec<u8> {
    build_zip_with_metadata(entries, &[], Some(timestamp))
}

fn build_zip_with_metadata(
    entries: &[TestEntry],
    comment: &[u8],
    timestamp: Option<DateTime>,
) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    writer
        .set_raw_comment(comment.to_vec().into_boxed_slice())
        .unwrap();
    for entry in entries {
        match entry {
            TestEntry::File {
                name,
                bytes,
                compression,
            } => {
                let mut options = SimpleFileOptions::default().compression_method(*compression);
                if let Some(timestamp) = timestamp {
                    options = options.last_modified_time(timestamp);
                }
                writer.start_file(name, options).unwrap();
                writer.write_all(bytes).unwrap();
            }
            TestEntry::Directory(name) => {
                let mut options = SimpleFileOptions::default();
                if let Some(timestamp) = timestamp {
                    options = options.last_modified_time(timestamp);
                }
                writer.add_directory(name, options).unwrap();
            }
        }
    }
    writer.finish().unwrap().into_inner()
}

pub(super) fn valid_entries() -> Vec<TestEntry> {
    vec![
        file("manifest.toml", b"api = 1".to_vec()),
        file("document.toml", b"id = 'example'".to_vec()),
        file("scripts/protocol.rhai", b"fn frame() {}".to_vec()),
    ]
}

pub(super) fn patch_raw_name_non_utf8(bytes: &mut [u8], name: &[u8]) {
    let mut replacements = 0;
    for offset in 0..=bytes.len().saturating_sub(name.len()) {
        if &bytes[offset..offset + name.len()] == name {
            bytes[offset] = 0xff;
            replacements += 1;
        }
    }
    assert_eq!(
        replacements, 2,
        "local and central names must both be patched"
    );
}

pub(super) fn patch_entry_name(bytes: &mut [u8], old: &[u8], new: &[u8]) {
    assert_eq!(old.len(), new.len());
    let mut replacements = 0;
    for offset in 0..=bytes.len().saturating_sub(old.len()) {
        if &bytes[offset..offset + old.len()] == old {
            bytes[offset..offset + old.len()].copy_from_slice(new);
            replacements += 1;
        }
    }
    assert_eq!(
        replacements, 2,
        "local and central names must both be patched"
    );
}

pub(super) fn patch_unix_mode(bytes: &mut [u8], name: &[u8], mode: u32) {
    let central = central_header_for_name(bytes, name);
    // ZIP "version made by" high byte 3 means Unix; external attributes high 16 bits carry st_mode.
    bytes[central + 4..central + 6].copy_from_slice(&[20, 3]);
    bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
}

pub(super) fn patch_encrypted(bytes: &mut [u8], name: &[u8]) {
    let local = local_header_for_name(bytes, name);
    set_u16_bit(bytes, local + 6, 0);
    let central = central_header_for_name(bytes, name);
    set_u16_bit(bytes, central + 8, 0);
}

pub(super) fn patch_compression(bytes: &mut [u8], name: &[u8], method: u16) {
    let local = local_header_for_name(bytes, name);
    bytes[local + 8..local + 10].copy_from_slice(&method.to_le_bytes());
    let central = central_header_for_name(bytes, name);
    bytes[central + 10..central + 12].copy_from_slice(&method.to_le_bytes());
}

pub(super) fn patch_central_uncompressed_size(bytes: &mut [u8], name: &[u8], size: u32) {
    let central = central_header_for_name(bytes, name);
    bytes[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
}

pub(super) fn patch_eocd_disk(bytes: &mut [u8], disk: u16) {
    let eocd = find_signature(bytes, [0x50, 0x4b, 0x05, 0x06], 0).expect("EOCD fixture signature");
    bytes[eocd + 4..eocd + 6].copy_from_slice(&disk.to_le_bytes());
}

pub(super) fn patch_second_entry_to_overlap_first(bytes: &mut [u8]) {
    let headers = central_headers(bytes);
    assert!(headers.len() >= 2);
    let first_offset = bytes[headers[0] + 42..headers[0] + 46].to_vec();
    bytes[headers[1] + 42..headers[1] + 46].copy_from_slice(&first_offset);
}

pub(super) fn corrupt_first_file_data(bytes: &mut [u8]) {
    let local = find_signature(bytes, [0x50, 0x4b, 0x03, 0x04], 0).unwrap();
    let name_len = read_u16(bytes, local + 26) as usize;
    let extra_len = read_u16(bytes, local + 28) as usize;
    let data = local + 30 + name_len + extra_len;
    bytes[data] ^= 0x5a;
}

fn central_headers(bytes: &[u8]) -> Vec<usize> {
    let mut headers = Vec::new();
    let mut start = 0;
    while let Some(offset) = find_signature(bytes, [0x50, 0x4b, 0x01, 0x02], start) {
        headers.push(offset);
        start = offset + 4;
    }
    headers
}

fn central_header_for_name(bytes: &[u8], name: &[u8]) -> usize {
    central_headers(bytes)
        .into_iter()
        .find(|offset| {
            let name_len = read_u16(bytes, *offset + 28) as usize;
            name_len == name.len() && &bytes[*offset + 46..*offset + 46 + name_len] == name
        })
        .expect("central header for fixture name")
}

fn local_header_for_name(bytes: &[u8], name: &[u8]) -> usize {
    let mut start = 0;
    loop {
        let offset = find_signature(bytes, [0x50, 0x4b, 0x03, 0x04], start)
            .expect("local header for fixture name");
        let name_len = read_u16(bytes, offset + 26) as usize;
        if name_len == name.len() && &bytes[offset + 30..offset + 30 + name_len] == name {
            return offset;
        }
        start = offset + 4;
    }
}

fn find_signature(bytes: &[u8], signature: [u8; 4], start: usize) -> Option<usize> {
    bytes[start..]
        .windows(signature.len())
        .position(|window| window == signature)
        .map(|offset| start + offset)
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn set_u16_bit(bytes: &mut [u8], offset: usize, bit: u32) {
    let value = read_u16(bytes, offset) | (1_u16 << bit);
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
