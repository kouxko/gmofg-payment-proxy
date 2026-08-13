//! Cross-target compilation gate for Windows-only infrastructure source.
//!
//! The full workspace cannot be cross-compiled from macOS without an MSVC C toolchain because
//! TLS dependencies compile native code. This small crate includes the exact platform source that
//! otherwise disappears behind `cfg(windows)`, so local Clippy catches Windows-only regressions.

use std::{io, path::PathBuf};

#[derive(Debug)]
pub enum InfrastructureError {
    DpapiUnsupported,
    DpapiProtect,
    DpapiUnprotect,
    ImportTooLarge {
        path: PathBuf,
        max_bytes: u64,
        actual_bytes: Option<u64>,
    },
    Import {
        path: PathBuf,
        source: io::Error,
    },
    Export {
        path: PathBuf,
        source: io::Error,
    },
    ExportTargetExists {
        path: PathBuf,
    },
}

#[path = "../../../src-tauri/crates/infrastructure/src/dpapi.rs"]
pub mod dpapi;

#[path = "../../../src-tauri/crates/infrastructure/src/files.rs"]
pub mod files;

#[path = "../../../src-tauri/crates/infrastructure/src/windows_process.rs"]
pub mod windows_process;

pub use dpapi::{DpapiProtector, SecretProtector};
pub use files::{AtomicFileExporter, ExportOutcome};
pub use windows_process::configure_background_process;

pub const IMPORT_LIMITS: [u64; 3] = [
    files::RULE_IMPORT_MAX_BYTES,
    files::PKCS12_IMPORT_MAX_BYTES,
    files::CA_IMPORT_MAX_BYTES,
];
