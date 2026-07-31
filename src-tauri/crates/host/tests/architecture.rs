//! 依赖方向的“防回退”测试。
//!
//! 这些测试扫描 Cargo 元数据和源码，防止 Tauri、Payment 产品词汇或产品编码重新渗入
//! 通用核心。它们证明的是静态架构边界，不证明运行时网络行为。

use std::path::{Path, PathBuf};
use std::process::Command;

const CORE_CRATES: [(&str, &str); 7] = [
    ("domain", "gmofg-proxy-domain"),
    ("application", "gmofg-proxy-application"),
    ("proxy", "gmofg-proxy-runtime"),
    ("product-api", "gmofg-proxy-product-api"),
    ("product-payment", "gmofg-proxy-product-payment"),
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

#[test]
fn generic_core_does_not_depend_on_concrete_payment_product() {
    for crate_name in [
        "gmofg-proxy-domain",
        "gmofg-proxy-application",
        "gmofg-proxy-runtime",
        "gmofg-proxy-product-api",
        "gmofg-proxy-infrastructure",
        "gmofg-proxy-host",
    ] {
        let packages = resolved_dependencies(crate_name);
        assert!(
            !packages
                .iter()
                .any(|package| package == "gmofg-proxy-product-payment"),
            "{crate_name} must not depend on the concrete Payment product: {packages:?}"
        );
    }
}

#[test]
fn generic_core_does_not_resolve_product_body_codecs() {
    for crate_name in [
        "gmofg-proxy-domain",
        "gmofg-proxy-application",
        "gmofg-proxy-runtime",
        "gmofg-proxy-infrastructure",
        "gmofg-proxy-host",
    ] {
        let packages = resolved_dependencies(crate_name);
        assert!(
            !packages.iter().any(|package| package == "encoding_rs"),
            "{crate_name} must receive body encoding through product-api, not resolve encoding_rs: {packages:?}"
        );
    }
}

#[test]
fn payment_product_library_does_not_pull_runtime_or_probe_dependencies() {
    let packages = resolved_dependencies("gmofg-proxy-product-payment");
    let forbidden = [
        "async-trait",
        "gmofg-proxy-infrastructure",
        "gmofg-proxy-runtime",
        "p12-keystore",
        "ring",
        "tokio",
        "tokio-util",
        "zeroize",
    ];

    for package in forbidden {
        assert!(
            !packages.iter().any(|resolved| resolved == package),
            "the default Payment product library must stay limited to product policy and codecs; \
             {package} belongs to the opt-in real-device probe: {packages:?}"
        );
    }
}

#[test]
fn generic_production_sources_do_not_contain_payment_contracts() {
    let forbidden = [
        "GMO-FG",
        "Payment App",
        "SHIFT_JIS",
        "shift_jis",
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

    for directory in ["domain", "application", "proxy", "infrastructure", "host"] {
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
