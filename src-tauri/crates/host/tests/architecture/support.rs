use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("host crate has crates parent")
        .to_path_buf()
}

pub(super) fn rust_sources(root: &Path) -> Vec<PathBuf> {
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

pub(super) fn is_test_source(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "tests")
        || path.file_stem().is_some_and(|stem| {
            let stem = stem.to_string_lossy();
            stem == "tests" || stem.ends_with("_tests")
        })
}

/// Removes items guarded by an exact `#[cfg(test)]` attribute while retaining
/// every production item before and after them.
///
/// A previous prefix-only scan stopped at the first test-only import or
/// constant, which allowed the rest of a production file to bypass the
/// product-contract guard. This small lexer handles Rust comments, ordinary
/// strings, raw strings, character literals, and balanced delimiters so the
/// architecture test cannot be bypassed by item placement.
pub(super) fn remove_cfg_test_items(source: &str) -> String {
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
            b';' | b',' if braces == 0 && parentheses == 0 && brackets == 0 => {
                // `#[cfg(test)]` 也可修饰 struct/enum 字段。字段以逗号结束，必须像
                // 分号结尾的 test-only item 一样完整剥离，否则后续生产源码会漏扫。
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

pub(super) fn assert_no_tauri_dependency(manifest_path: &Path, manifest: &str) {
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

pub(super) fn resolved_dependencies(crate_name: &str) -> Vec<String> {
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
