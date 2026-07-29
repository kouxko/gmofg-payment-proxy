use std::path::{Path, PathBuf};
use std::process::Command;

const CORE_CRATES: [(&str, &str); 5] = [
    ("domain", "gmofg-proxy-domain"),
    ("application", "gmofg-proxy-application"),
    ("proxy", "gmofg-proxy-runtime"),
    ("infrastructure", "gmofg-proxy-infrastructure"),
    ("host", "gmofg-proxy-host"),
];

#[test]
fn reusable_rust_crates_do_not_depend_on_tauri() {
    let crates_dir = crates_dir();

    for (directory, package_name) in CORE_CRATES {
        let manifest_path = crates_dir.join(directory).join("Cargo.toml");
        let manifest = std::fs::read_to_string(&manifest_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", manifest_path.display()));
        assert_no_tauri_dependency(&manifest_path, &manifest);

        let packages = resolved_dependencies(package_name);
        assert!(
            packages.iter().all(|package| !package.starts_with("tauri")),
            "{package_name} must stay UI-neutral, resolved dependencies include: {packages:?}"
        );
    }
}

#[test]
fn runtime_crate_does_not_depend_on_application_layer() {
    let packages = resolved_dependencies("gmofg-proxy-runtime");
    assert!(
        !packages
            .iter()
            .any(|package| package == "gmofg-proxy-application"),
        "proxy runtime must not depend upward on application: {packages:?}"
    );
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("host crate has crates parent")
        .to_path_buf()
}

fn assert_no_tauri_dependency(manifest_path: &Path, manifest: &str) {
    let dependency = manifest.lines().map(str::trim).find(|line| {
        line.starts_with("tauri =") || line.starts_with("tauri-") || line.starts_with("tauri_")
    });
    assert!(
        dependency.is_none(),
        "{} must stay UI-neutral, found dependency: {}",
        manifest_path.display(),
        dependency.unwrap_or_default()
    );
}

fn resolved_dependencies(crate_name: &str) -> Vec<String> {
    let workspace_manifest = crates_dir()
        .parent()
        .expect("crates directory has workspace parent")
        .join("Cargo.toml");
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([
            "tree",
            "--manifest-path",
            workspace_manifest
                .to_str()
                .expect("workspace manifest is valid UTF-8"),
            "--package",
            crate_name,
            "--edges",
            "normal,build",
            "--prefix",
            "none",
        ])
        .output()
        .unwrap_or_else(|error| panic!("run cargo tree for {crate_name}: {error}"));
    assert!(
        output.status.success(),
        "cargo tree failed for {crate_name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("cargo tree output is UTF-8")
        .lines()
        .filter_map(|line| line.split_ascii_whitespace().next())
        .map(ToOwned::to_owned)
        .collect()
}
