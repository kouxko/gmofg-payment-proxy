use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;

use crate::InfrastructureError;

pub(crate) const RULE_IMPORT_MAX_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const PKCS12_IMPORT_MAX_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const CA_IMPORT_MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    pub path: PathBuf,
    pub bytes_written: u64,
    pub replaced_existing: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AtomicFileExporter;

impl AtomicFileExporter {
    /// Writes a sibling temporary file, syncs it, then persists it into place.
    /// Dropping the temporary file removes it automatically after failures.
    pub fn write(
        &self,
        path: &Path,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<ExportOutcome, InfrastructureError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let existed = path.exists();
        if existed && !overwrite {
            return Err(InfrastructureError::ExportTargetExists {
                path: path.to_path_buf(),
            });
        }

        let mut temporary =
            NamedTempFile::new_in(parent).map_err(|source| InfrastructureError::Export {
                path: path.to_path_buf(),
                source,
            })?;
        temporary
            .write_all(bytes)
            .map_err(|source| InfrastructureError::Export {
                path: path.to_path_buf(),
                source,
            })?;
        temporary
            .as_file_mut()
            .sync_all()
            .map_err(|source| InfrastructureError::Export {
                path: path.to_path_buf(),
                source,
            })?;

        if overwrite {
            temporary.persist(path)
        } else {
            temporary.persist_noclobber(path)
        }
        .map_err(|error| InfrastructureError::Export {
            path: path.to_path_buf(),
            source: error.error,
        })?;

        sync_parent(parent, path)?;
        Ok(ExportOutcome {
            path: path.to_path_buf(),
            bytes_written: bytes.len() as u64,
            replaced_existing: existed,
        })
    }

    pub fn read_bounded(
        &self,
        path: &Path,
        max_bytes: u64,
    ) -> Result<Vec<u8>, InfrastructureError> {
        let file = File::open(path).map_err(|source| InfrastructureError::Import {
            path: path.to_path_buf(),
            source,
        })?;
        let metadata = file
            .metadata()
            .map_err(|source| InfrastructureError::Import {
                path: path.to_path_buf(),
                source,
            })?;
        if metadata.len() > max_bytes {
            return Err(InfrastructureError::ImportTooLarge {
                path: path.to_path_buf(),
                max_bytes,
                actual_bytes: Some(metadata.len()),
            });
        }

        let read_limit = max_bytes.saturating_add(1);
        let mut bytes =
            Vec::with_capacity(usize::try_from(metadata.len().min(max_bytes)).unwrap_or_default());
        file.take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|source| InfrastructureError::Import {
                path: path.to_path_buf(),
                source,
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(InfrastructureError::ImportTooLarge {
                path: path.to_path_buf(),
                max_bytes,
                actual_bytes: None,
            });
        }
        Ok(bytes)
    }
}

fn sync_parent(parent: &Path, target: &Path) -> Result<(), InfrastructureError> {
    #[cfg(unix)]
    {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| InfrastructureError::Export {
                path: target.to_path_buf(),
                source,
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SECURITY-012, SECURITY-013: explicit overwrite is required and failure
    /// leaves the previous file intact.
    #[test]
    fn atomic_export_requires_overwrite_confirmation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("sessions.json");
        let exporter = AtomicFileExporter;
        exporter.write(&path, b"old", false).expect("first write");

        assert!(matches!(
            exporter.write(&path, b"new", false),
            Err(InfrastructureError::ExportTargetExists { .. })
        ));
        assert_eq!(std::fs::read(&path).expect("read"), b"old");

        let outcome = exporter.write(&path, b"new", true).expect("replace");
        assert!(outcome.replaced_existing);
        assert_eq!(std::fs::read(&path).expect("read"), b"new");
    }

    #[test]
    fn bounded_import_accepts_exact_limit() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("rules.json");
        std::fs::write(&path, [b'x'; 16]).expect("write boundary file");

        assert_eq!(
            AtomicFileExporter
                .read_bounded(&path, 16)
                .expect("read exact boundary"),
            vec![b'x'; 16]
        );
    }

    #[test]
    fn bounded_import_rejects_metadata_over_limit_with_stable_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("rules.json");
        std::fs::write(&path, [b'x'; 17]).expect("write oversized file");

        let error = AtomicFileExporter
            .read_bounded(&path, 16)
            .expect_err("reject oversized import");
        assert!(matches!(
            error,
            InfrastructureError::ImportTooLarge {
                max_bytes: 16,
                actual_bytes: Some(17),
                ..
            }
        ));
        assert_eq!(error.code(), crate::InfrastructureErrorCode::ImportTooLarge);
    }

    #[test]
    fn each_import_type_accepts_its_exact_explicit_limit() {
        let directory = tempfile::tempdir().expect("tempdir");
        for (name, limit) in [
            ("rules.json", RULE_IMPORT_MAX_BYTES),
            ("identity.p12", PKCS12_IMPORT_MAX_BYTES),
            ("upstream-ca.crt", CA_IMPORT_MAX_BYTES),
        ] {
            let path = directory.path().join(name);
            File::create(&path)
                .expect("create boundary file")
                .set_len(limit)
                .expect("set boundary size");
            let bytes = AtomicFileExporter
                .read_bounded(&path, limit)
                .expect("exact per-type boundary");
            assert_eq!(u64::try_from(bytes.len()).expect("length"), limit);
        }
        const {
            assert!(CA_IMPORT_MAX_BYTES < RULE_IMPORT_MAX_BYTES);
            assert!(RULE_IMPORT_MAX_BYTES < PKCS12_IMPORT_MAX_BYTES);
        }
    }
}
