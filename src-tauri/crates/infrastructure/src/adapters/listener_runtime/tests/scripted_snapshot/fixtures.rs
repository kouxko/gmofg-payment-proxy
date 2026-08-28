use super::*;

pub(super) fn snapshot_zip(script: &str) -> Vec<u8> {
    snapshot_zip_with_manifest(SNAPSHOT_MANIFEST, script)
}

pub(super) fn snapshot_zip_with_manifest(manifest: &str, script: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", manifest.as_bytes()),
        ("document.toml", SNAPSHOT_SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
        ("display.rhai", SNAPSHOT_DISPLAY.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
