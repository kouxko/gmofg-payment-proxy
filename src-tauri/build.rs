//! Tauri 构建脚本入口，用于生成平台资源和 Cargo 构建元数据。
//!
//! 它在编译期运行，不参与代理运行时，也不得读取或生成用户证书材料。

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const BUILTIN_SOURCE: &str = "../templates/socket-protocol/iso8583-standard";
const BUILTIN_ARCHIVE: &str = "iso8583-ascii-standard-1.0.0.zip";

fn main() {
    let source = PathBuf::from(BUILTIN_SOURCE);
    println!("cargo:rerun-if-changed={}", source.display());
    build_builtin_archive(&source).expect("failed to build built-in ISO 8583 protocol package");
    tauri_build::build();
}

fn build_builtin_archive(source: &Path) -> io::Result<()> {
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"))
        .join(BUILTIN_ARCHIVE);
    let mut files = Vec::new();
    collect_files(source, source, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let file = fs::File::create(output)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    for (relative, path) in files {
        archive.start_file(relative, options)?;
        archive.write_all(&fs::read(path)?)?;
    }
    archive.finish()?;
    Ok(())
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("collected file must remain below package root")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path));
        }
    }
    Ok(())
}
