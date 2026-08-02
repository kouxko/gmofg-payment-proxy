//! 依赖方向的“防回退”测试。
//!
//! 这些测试扫描 Cargo 元数据和源码，防止 Tauri 或已删除的旧产品契约重新渗入通用核心，
//! 同时固定动态 Workspace 编解码器的依赖方向。它们不证明运行时网络行为。

use std::path::{Path, PathBuf};
use std::process::Command;

const CORE_CRATES: [(&str, &str); 7] = [
    ("domain", "intercept-proxy-domain"),
    ("application", "intercept-proxy-application"),
    ("android-engine", "intercept-proxy-android-engine"),
    ("proxy", "intercept-proxy-runtime"),
    ("product-api", "intercept-proxy-product-api"),
    ("infrastructure", "intercept-proxy-infrastructure"),
    ("host", "intercept-proxy-host"),
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
    let packages = resolved_dependencies("intercept-proxy-runtime");
    assert!(
        !packages
            .iter()
            .any(|package| package == "intercept-proxy-application"),
        "proxy runtime must not depend upward on application: {packages:?}"
    );
}

#[test]
fn generic_core_does_not_depend_on_removed_legacy_product_fixture() {
    for crate_name in [
        "intercept-proxy-domain",
        "intercept-proxy-application",
        "intercept-proxy-android-engine",
        "intercept-proxy-runtime",
        "intercept-proxy-product-api",
        "intercept-proxy-infrastructure",
        "intercept-proxy-host",
    ] {
        let packages = resolved_dependencies(crate_name);
        assert!(
            !packages
                .iter()
                .any(|package| package == "intercept-proxy-legacy-test-fixture"),
            "{crate_name} must not depend on the removed legacy product fixture: {packages:?}"
        );
    }
}

#[test]
fn dynamic_workspace_body_codecs_stay_in_infrastructure() {
    for crate_name in [
        "intercept-proxy-domain",
        "intercept-proxy-application",
        "intercept-proxy-runtime",
    ] {
        let packages = resolved_dependencies(crate_name);
        assert!(
            !packages.iter().any(|package| package == "encoding_rs"),
            "{crate_name} must remain independent of concrete text encodings: {packages:?}"
        );
    }
    let infrastructure = resolved_dependencies("intercept-proxy-infrastructure");
    assert!(
        infrastructure
            .iter()
            .any(|package| package == "encoding_rs"),
        "infrastructure must implement the Workspace-selectable Shift-JIS codec: {infrastructure:?}"
    );
}

#[test]
fn removed_legacy_product_fixture_is_not_a_workspace_member() {
    let manifest = std::fs::read_to_string(crates_dir().parent().unwrap().join("Cargo.toml"))
        .expect("read workspace manifest");
    assert!(!manifest.contains("product-payment"));
    assert!(!crates_dir().join("product-payment").exists());
}

#[test]
fn generic_production_sources_do_not_contain_removed_product_contracts() {
    let forbidden = [
        "GMO-FG",
        "Payment App",
        "D48",
        "ChannelKind::Transaction",
        "ChannelKind::Dll",
        "enum ChannelKind",
        "transaction_port",
        "dll_port",
        "upstream_transaction_url",
        "upstream_dll_url",
        "16_627",
        "16_127",
        "gmofg-payment-proxy.sqlite3",
        "com.gmofg.payment-proxy",
        "gmofg-payment-proxy/keychain",
    ];

    for directory in [
        "domain",
        "application",
        "android-engine",
        "proxy",
        "infrastructure",
        "host",
    ] {
        let source_dir = crates_dir().join(directory).join("src");
        for source in rust_sources(&source_dir) {
            let text = std::fs::read_to_string(&source)
                .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
            let production = remove_cfg_test_items(&text);
            for term in forbidden {
                assert!(
                    !production.contains(term),
                    "{} generic production source contains product contract {term:?}",
                    source.display()
                );
            }
        }
    }
}

#[test]
fn product_contract_scan_keeps_production_after_test_only_items() {
    let source = r####"
const BEFORE: &str = "generic";
#[cfg(test)]
const TEST_ONLY: &str = "GMO-FG";
const AFTER_CONST: &str = "production-after-const";
#[cfg(test)]
fn helper() {
    let raw = r###"GMO-FG { test }"###;
    assert!(!raw.is_empty());
}
const AFTER_FUNCTION: &str = "production-after-function";
#[cfg(test)]
mod tests {
    const FIXTURE: &str = "GMO-FG";
}
const AFTER_MODULE: &str = "production-after-module";
"####;

    let production = remove_cfg_test_items(source);

    assert!(!production.contains("GMO-FG"));
    assert!(production.contains("production-after-const"));
    assert!(production.contains("production-after-function"));
    assert!(production.contains("production-after-module"));
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("host crate has crates parent")
        .to_path_buf()
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read directory {}: {error}", directory.display()));
        for entry in entries {
            let path = entry.expect("valid directory entry").path();
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

/// Removes items guarded by an exact `#[cfg(test)]` attribute while retaining
/// every production item before and after them.
///
/// A previous prefix-only scan stopped at the first test-only import or
/// constant, which allowed the rest of a production file to bypass the
/// product-contract guard. This small lexer handles Rust comments, ordinary
/// strings, raw strings, character literals, and balanced delimiters so the
/// architecture test cannot be bypassed by item placement.
fn remove_cfg_test_items(source: &str) -> String {
    const ATTRIBUTE: &str = "#[cfg(test)]";

    let mut production = String::with_capacity(source.len());
    let mut cursor = 0;
    while let Some(relative_start) = source[cursor..].find(ATTRIBUTE) {
        let attribute_start = cursor + relative_start;
        production.push_str(&source[cursor..attribute_start]);
        let item_start = attribute_start + ATTRIBUTE.len();
        let item_end = cfg_test_item_end(source, item_start).unwrap_or_else(|| {
            panic!("unable to find end of cfg(test) item near byte {attribute_start}")
        });
        cursor = item_end;
    }
    production.push_str(&source[cursor..]);
    production
}

fn cfg_test_item_end(source: &str, after_attribute: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = after_attribute;
    let mut braces = 0_u32;
    let mut parentheses = 0_u32;
    let mut brackets = 0_u32;
    let mut saw_block = false;

    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = skip_line_comment(bytes, index + 2);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_block_comment(bytes, index + 2)?;
            }
            b'"' => {
                index = skip_quoted(bytes, index + 1, b'"')?;
            }
            b'\'' if is_character_literal(bytes, index) => {
                index = skip_quoted(bytes, index + 1, b'\'')?;
            }
            b'r' if raw_string_hashes(bytes, index).is_some() => {
                let hashes = raw_string_hashes(bytes, index)?;
                index = skip_raw_string(bytes, index, hashes)?;
            }
            b'{' => {
                braces = braces.saturating_add(1);
                saw_block = true;
                index += 1;
            }
            b'}' => {
                braces = braces.checked_sub(1)?;
                index += 1;
                if saw_block && braces == 0 && parentheses == 0 && brackets == 0 {
                    return Some(index);
                }
            }
            b'(' => {
                parentheses = parentheses.saturating_add(1);
                index += 1;
            }
            b')' => {
                parentheses = parentheses.checked_sub(1)?;
                index += 1;
            }
            b'[' => {
                brackets = brackets.saturating_add(1);
                index += 1;
            }
            b']' => {
                brackets = brackets.checked_sub(1)?;
                index += 1;
            }
            b';' if braces == 0 && parentheses == 0 && brackets == 0 => {
                return Some(index + 1);
            }
            _ => index += 1,
        }
    }
    None
}

fn skip_line_comment(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

fn skip_block_comment(bytes: &[u8], mut index: usize) -> Option<usize> {
    let mut depth = 1_u32;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"/*") {
            depth = depth.saturating_add(1);
            index += 2;
        } else if bytes[index..].starts_with(b"*/") {
            depth = depth.checked_sub(1)?;
            index += 2;
            if depth == 0 {
                return Some(index);
            }
        } else {
            index += 1;
        }
    }
    None
}

fn skip_quoted(bytes: &[u8], mut index: usize, quote: u8) -> Option<usize> {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = index.saturating_add(2),
            byte if byte == quote => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

fn is_character_literal(bytes: &[u8], index: usize) -> bool {
    matches!(
        (
            bytes.get(index + 1),
            bytes.get(index + 2),
            bytes.get(index + 3),
        ),
        (Some(b'\\'), Some(_), Some(b'\'')) | (Some(_), Some(b'\''), _)
    )
}

fn raw_string_hashes(bytes: &[u8], index: usize) -> Option<usize> {
    let mut cursor = index + 1;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'"')).then_some(cursor - index - 1)
}

fn skip_raw_string(bytes: &[u8], index: usize, hashes: usize) -> Option<usize> {
    let content_start = index + 2 + hashes;
    let mut cursor = content_start;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes
                .get(cursor + 1..cursor + 1 + hashes)
                .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Some(cursor + 1 + hashes);
        }
        cursor += 1;
    }
    None
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
