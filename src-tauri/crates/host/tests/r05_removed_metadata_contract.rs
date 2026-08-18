use std::path::{Path, PathBuf};

#[test]
fn production_http_session_and_capture_consumers_do_not_expose_removed_metadata_fields() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("host crate has crates parent");
    let mut violations = Vec::new();
    for crate_name in ["application", "domain", "infrastructure", "proxy", "host"] {
        for source in rust_sources(&crates.join(crate_name).join("src")) {
            if is_test_source(&source) || is_legacy_migration_boundary(&source) {
                continue;
            }
            let text = std::fs::read_to_string(&source)
                .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
            for removed in ["extracted_metadata", "metadata_extractors"] {
                if contains_identifier(&text, removed) {
                    violations.push(format!("{} contains {removed}", source.display()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "removed metadata fields escaped their migration-only boundary:\n{}",
        violations.join("\n")
    );
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
}

fn is_test_source(path: &Path) -> bool {
    path.components().any(|component| {
        let component = component.as_os_str().to_string_lossy();
        component == "tests" || component.ends_with("_tests")
    }) || path.file_stem().is_some_and(|stem| {
        let stem = stem.to_string_lossy();
        stem == "tests" || stem.ends_with("_tests")
    })
}

fn is_legacy_migration_boundary(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("workspace_migration.rs" | "workspace_documents.rs")
    )
}

fn contains_identifier(source: &str, identifier: &str) -> bool {
    source.match_indices(identifier).any(|(start, _)| {
        let before = source[..start].chars().next_back();
        let after = source[start + identifier.len()..].chars().next();
        !before.is_some_and(is_identifier_character) && !after.is_some_and(is_identifier_character)
    })
}

fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}
