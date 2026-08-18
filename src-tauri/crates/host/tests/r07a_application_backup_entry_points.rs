use std::path::{Path, PathBuf};

#[test]
fn application_backup_zip_has_no_tauri_command_in_r07a() {
    let host = read_tree(&repository_root().join("src-tauri/src"), "rs");

    for forbidden in [
        "application_archive_import",
        "application_archive_export",
        "application_backup_import",
        "application_backup_export",
        "read_application_archive_zip",
    ] {
        assert!(
            !host.contains(forbidden),
            "R07a must not expose the application backup ZIP through Tauri: {forbidden}"
        );
    }
}

#[test]
fn application_backup_zip_has_no_frontend_entry_in_r07a() {
    let frontend = read_tree(&repository_root().join("src"), "tsx");

    for forbidden in [
        "导入应用数据",
        "导出应用数据",
        "applicationArchiveImport",
        "applicationArchiveExport",
        "applicationBackupImport",
        "applicationBackupExport",
        "intercept-proxy-backup-",
    ] {
        assert!(
            !frontend.contains(forbidden),
            "R07a must keep the application backup ZIP UI closed: {forbidden}"
        );
    }
}

#[test]
fn legacy_json_configuration_paths_remain_registered_in_r07a() {
    let command_module =
        std::fs::read_to_string(repository_root().join("src-tauri/src/commands/mod.rs"))
            .expect("read Tauri command module");
    let generated_bindings =
        std::fs::read_to_string(repository_root().join("src/generated/rust-types.ts"))
            .expect("read generated bindings");

    for legacy_command in [
        "application_configuration_import",
        "application_configuration_export",
        "workspace_import",
        "workspace_export",
    ] {
        assert!(
            command_module.contains(legacy_command),
            "R07a must not remove legacy JSON command {legacy_command}"
        );
    }
    for legacy_binding in [
        "applicationConfigurationImport",
        "applicationConfigurationExport",
        "workspaceImport",
        "workspaceExport",
    ] {
        assert!(
            generated_bindings.contains(legacy_binding),
            "R07a must not remove legacy JSON binding {legacy_binding}"
        );
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("host crate is nested under src-tauri/crates")
        .to_path_buf()
}

fn read_tree(root: &Path, extension: &str) -> String {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|value| value == extension) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|path| {
            std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
