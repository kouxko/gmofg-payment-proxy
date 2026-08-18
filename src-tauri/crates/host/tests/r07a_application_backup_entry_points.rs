use std::path::{Path, PathBuf};

#[test]
fn application_backup_zip_commands_are_registered() {
    let command_module =
        std::fs::read_to_string(repository_root().join("src-tauri/src/commands/mod.rs"))
            .expect("read Tauri command module");

    for command in [
        "application_backup_export",
        "application_backup_import_prepare",
        "application_backup_import_commit",
        "application_backup_import_discard",
    ] {
        assert!(
            command_module.contains(command),
            "application backup ZIP command is not registered: {command}"
        );
    }
}

#[test]
fn application_backup_zip_has_one_frontend_import_and_export_flow() {
    let frontend = read_tree(&repository_root().join("src"), "tsx");

    for required in [
        "导入应用数据",
        "导出应用数据",
        "applicationBackupImportPrepare",
        "applicationBackupImportCommit",
        "applicationBackupImportDiscard",
        "applicationBackupExport",
    ] {
        assert!(
            frontend.contains(required),
            "application backup ZIP UI flow is missing: {required}"
        );
    }
}

#[test]
fn legacy_json_configuration_paths_are_not_registered() {
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
            !command_module.contains(legacy_command),
            "legacy JSON command must stay removed: {legacy_command}"
        );
    }
    for legacy_binding in [
        "applicationConfigurationImport",
        "applicationConfigurationExport",
        "workspaceImport",
        "workspaceExport",
    ] {
        assert!(
            !generated_bindings.contains(legacy_binding),
            "legacy JSON binding must stay removed: {legacy_binding}"
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
