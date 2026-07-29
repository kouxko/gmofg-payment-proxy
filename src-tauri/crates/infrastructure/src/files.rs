use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;

use crate::InfrastructureError;

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

    pub fn read(&self, path: &Path) -> Result<Vec<u8>, InfrastructureError> {
        std::fs::read(path).map_err(|source| InfrastructureError::Import {
            path: path.to_path_buf(),
            source,
        })
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
}
