//! Package archive validation boundary.
#![deny(missing_docs)]

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use intercept_proxy_domain::{DomainError, ErrorCode};
use intercept_proxy_package_contract::PackageManifest;
use zip::ZipArchive;

const REQUIRED_ROOT_FILES: [&str; 3] = ["manifest.json", "protocol.js", "display.js"];

/// Confirmed package archive resource limits supplied by the host configuration owner.
pub trait PackageArchiveResourceLimits {
    /// Compressed ZIP byte limit.
    fn max_archive_bytes(&self) -> u64;
    /// Central-directory entry limit.
    fn max_entries(&self) -> usize;
    /// Per-file expanded byte limit.
    fn max_file_bytes(&self) -> u64;
    /// Total expanded byte limit.
    fn max_total_bytes(&self) -> u64;
    /// Per-file expanded/compressed ratio limit.
    fn max_compression_ratio(&self) -> u64;
    /// Package-relative path segment limit.
    fn max_path_depth(&self) -> usize;
}

/// A validated package ZIP containing the strict shared Manifest and JavaScript modules.
#[derive(Clone, Debug)]
pub struct PackageArchive {
    manifest: PackageManifest,
    files: BTreeMap<String, Vec<u8>>,
}

impl PackageArchive {
    /// Returns the strict API 1 Manifest parsed from the root `manifest.json`.
    #[must_use]
    pub const fn manifest(&self) -> &PackageManifest {
        &self.manifest
    }

    /// Returns one validated package-relative JavaScript file.
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Iterates validated files in deterministic path order.
    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }
}

fn invalid(field: &str, message: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::ProtocolPackageInvalid, "package ZIP is invalid")
        .with_field_error(field, message)
}

fn validate_path(path: &str) -> Result<(), DomainError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        || Path::new(path).extension() != Some(OsStr::new("js")) && path != "manifest.json"
    {
        return Err(invalid(
            "archive.path",
            format!("unsupported package path: {path}"),
        ));
    }
    Ok(())
}

/// Reads a package ZIP without extracting it to the filesystem.
///
/// The root has exactly one strict `manifest.json`, `protocol.js`, and `display.js`; every other
/// file is a package-relative `.js` module. JavaScript parsing and ESM linking belong to Phase 8.
pub fn read_package_zip<R: Read + Seek, L: PackageArchiveResourceLimits>(
    mut reader: R,
    limits: &L,
) -> Result<PackageArchive, DomainError> {
    let archive_bytes = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| invalid("archive", "cannot measure ZIP"))?;
    if archive_bytes > limits.max_archive_bytes() {
        return Err(invalid("archive", "ZIP exceeds archive byte limit"));
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| invalid("archive", "cannot rewind ZIP"))?;
    let mut archive = ZipArchive::new(reader).map_err(|_| invalid("archive", "invalid ZIP"))?;
    if archive.len() > limits.max_entries() {
        return Err(invalid("archive", "ZIP exceeds entry limit"));
    }
    let mut files = BTreeMap::new();
    let mut total_bytes = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| invalid("archive", "invalid ZIP entry"))?;
        let path = entry.name().to_owned();
        if !entry.is_file()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
        {
            return Err(invalid(
                "archive.path",
                format!("only regular files are allowed: {path}"),
            ));
        }
        validate_path(&path)?;
        if path.split('/').count() > limits.max_path_depth() {
            return Err(invalid("archive.path", "package path exceeds depth limit"));
        }
        let declared_size = entry.size();
        if declared_size > limits.max_file_bytes() {
            return Err(invalid("archive", "ZIP entry exceeds file byte limit"));
        }
        let compressed = entry.compressed_size();
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(limits.max_file_bytes().saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| invalid("archive", "cannot read ZIP entry"))?;
        let actual_size = u64::try_from(bytes.len())
            .map_err(|_| invalid("archive", "ZIP entry byte size overflow"))?;
        if actual_size > limits.max_file_bytes() {
            return Err(invalid("archive", "ZIP entry exceeds file byte limit"));
        }
        if actual_size != declared_size {
            return Err(invalid(
                "archive",
                "ZIP entry declared and actual byte sizes differ",
            ));
        }
        total_bytes = total_bytes
            .checked_add(actual_size)
            .ok_or_else(|| invalid("archive", "ZIP total byte size overflow"))?;
        if total_bytes > limits.max_total_bytes() {
            return Err(invalid("archive", "ZIP exceeds total byte limit"));
        }
        if actual_size > 0
            && (compressed == 0
                || actual_size > compressed.saturating_mul(limits.max_compression_ratio()))
        {
            return Err(invalid(
                "archive",
                "ZIP entry exceeds compression ratio limit",
            ));
        }
        if files.insert(path.clone(), bytes).is_some() {
            return Err(invalid(
                "archive.path",
                format!("duplicate package path: {path}"),
            ));
        }
    }
    for required in REQUIRED_ROOT_FILES {
        if !files.contains_key(required) {
            return Err(invalid("archive", format!("missing root {required}")));
        }
    }
    let manifest = serde_json::from_slice::<PackageManifest>(&files["manifest.json"])
        .map_err(|error| invalid("manifest.json", error.to_string()))?;
    Ok(PackageArchive { manifest, files })
}
