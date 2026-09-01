//! Tauri build entry point and reproducible built-in Wasm Component compilation.

use std::{borrow::Cow, env, fs, path::PathBuf, process::Command};

use wasm_encoder::{ComponentSection, CustomSection};

const BUILTIN_SOURCE: &str = "../templates/socket-protocol/iso8583-standard";
const BUILTIN_COMPONENT: &str = "iso8583-ascii-standard-1.0.0.wasm";
const BUILTIN_ARTIFACT: &str = "intercept_proxy_iso8583_ascii_standard_component.wasm";

fn main() {
    let source = PathBuf::from(BUILTIN_SOURCE);
    let wit = PathBuf::from("crates/package-runtime/wit");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed={}", wit.display());
    build_builtin_component(&source).expect("failed to build built-in ISO 8583 Wasm Component");
    tauri_build::build();
}

fn build_builtin_component(source: &std::path::Path) -> Result<(), String> {
    let output_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo must provide OUT_DIR")?);
    let target_dir = output_dir.join("builtin-component-target");
    let cargo = env::var_os("CARGO").ok_or("Cargo must provide its executable path")?;
    let status = Command::new(cargo)
        .arg("build")
        .arg("--locked")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip2")
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--manifest-path")
        .arg(source.join("Cargo.toml"))
        .status()
        .map_err(|error| format!("cannot start built-in Component build: {error}"))?;
    if !status.success() {
        return Err(format!("built-in Component build exited with {status}"));
    }
    let built = target_dir
        .join("wasm32-wasip2/release")
        .join(BUILTIN_ARTIFACT);
    let mut component =
        fs::read(&built).map_err(|error| format!("cannot read {}: {error}", built.display()))?;
    let manifest_path = source.join("manifest.json");
    let manifest = fs::read(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    serde_json::from_slice::<serde_json::Value>(&manifest)
        .map_err(|error| format!("invalid {}: {error}", manifest_path.display()))?;
    CustomSection {
        name: Cow::Borrowed("intercept-proxy:manifest"),
        data: Cow::Borrowed(&manifest),
    }
    .append_to_component(&mut component);
    fs::write(output_dir.join(BUILTIN_COMPONENT), component)
        .map_err(|error| format!("cannot stage {}: {error}", built.display()))?;
    Ok(())
}
