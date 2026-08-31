use std::{fs, io::Cursor, path::PathBuf};

use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult,
};
use intercept_proxy_package_runtime::{
    LocalSidecarRuntime, PackageArchiveResourceLimits, read_package_zip,
};
use zip::{ZipWriter, write::SimpleFileOptions};

#[derive(Debug)]
struct Limits;

impl PackageArchiveResourceLimits for Limits {
    fn max_archive_bytes(&self) -> u64 {
        8 * 1024 * 1024
    }
    fn max_entries(&self) -> usize {
        64
    }
    fn max_file_bytes(&self) -> u64 {
        1024 * 1024
    }
    fn max_total_bytes(&self) -> u64 {
        4 * 1024 * 1024
    }
    fn max_compression_ratio(&self) -> u64 {
        100
    }
    fn max_path_depth(&self) -> usize {
        8
    }
}

fn template_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../templates/socket-protocol/iso8583-standard")
}

fn template_archive() -> Vec<u8> {
    let root = template_root();
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut output);
        for path in ["manifest.json", "protocol.js", "display.js"] {
            writer
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, &fs::read(root.join(path)).unwrap()).unwrap();
        }
        writer.finish().unwrap();
    }
    output.into_inner()
}

#[test]
fn strict_builtin_archive_executes_frame_decode_display_and_encode() {
    let archive = read_package_zip(Cursor::new(template_archive()), &Limits).unwrap();
    assert_eq!(
        archive.manifest().package().identity().id.as_str(),
        "iso8583-ascii-standard"
    );
    let mut runtime = LocalSidecarRuntime::load(&archive).unwrap();

    let bytes = [0_u8, 4, b'0', b'8', b'0', b'0'];
    assert!(matches!(
        runtime
            .upstream_frame(FrameParams {
                buffer: CanonicalBase64::from_bytes(&bytes)
            })
            .unwrap(),
        FrameResult::Complete { .. }
    ));
    let document = runtime
        .upstream_decode(DecodeParams {
            input: CanonicalBase64::from_bytes(&bytes).as_str().to_owned(),
        })
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    assert_eq!(json["message_type"], "0800");
    let display = runtime
        .upstream_display(DisplayParams {
            document: document.clone(),
        })
        .unwrap();
    assert!(display.contains("ISO 8583:1987 Message"));
    assert!(display.contains("<td>0800</td>"));
    let encoded = runtime
        .upstream_encode(EncodeParams {
            original_input: CanonicalBase64::from_bytes(&bytes).as_str().to_owned(),
            document,
        })
        .unwrap();
    assert_eq!(CanonicalBase64::try_from(encoded).unwrap().bytes(), bytes);
}

#[test]
fn strict_builtin_archive_has_only_manifest_protocol_and_display() {
    let root = template_root();
    for path in [
        "manifest.toml",
        "document.toml",
        "protocol.rhai",
        "display.rhai",
    ] {
        assert!(!root.join(path).exists(), "legacy template remains: {path}");
    }
}
