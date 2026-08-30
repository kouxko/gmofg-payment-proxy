use std::io::{Cursor, Write};

use intercept_proxy_domain::Document;
use intercept_proxy_package_contract::DecodeParams;
use intercept_proxy_package_runtime::{
    LocalSidecarRuntime, PackageArchiveResourceLimits, read_package_zip,
};
use zip::{ZipWriter, write::SimpleFileOptions};

const HTTP_MANIFEST: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);
const DISPLAY: &str = r#"
    export function upstreamDisplay(params) { return ""; }
    export function downstreamDisplay(params) { return ""; }
"#;

#[derive(Default)]
struct Limits;

impl PackageArchiveResourceLimits for Limits {
    fn max_archive_bytes(&self) -> u64 {
        8 * 1024 * 1024
    }
    fn max_entries(&self) -> usize {
        64
    }
    fn max_file_bytes(&self) -> u64 {
        1024 * 1024
    }
    fn max_total_bytes(&self) -> u64 {
        4 * 1024 * 1024
    }
    fn max_compression_ratio(&self) -> u64 {
        100
    }
    fn max_path_depth(&self) -> usize {
        8
    }
}

fn archive(protocol: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut output);
        for (path, bytes) in [
            ("manifest.json", HTTP_MANIFEST.as_bytes()),
            ("protocol.js", protocol.as_bytes()),
            ("display.js", DISPLAY.as_bytes()),
        ]
        .into_iter()
        .chain(extra.iter().copied())
        {
            writer
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    output.into_inner()
}

fn load(protocol: &str, extra: &[(&str, &[u8])]) -> LocalSidecarRuntime {
    let package = read_package_zip(Cursor::new(archive(protocol, extra)), &Limits).unwrap();
    LocalSidecarRuntime::load(&package).unwrap()
}

#[test]
fn dynamic_import_uses_boa_loader_and_evaluates_the_lazy_module_once() {
    let protocol = r#"
        export async function upstreamDecode(params) {
            const lazy = await import("./lib/lazy.js");
            return { value: lazy.value, evaluations: globalThis.lazyEvaluations };
        }
        export function downstreamDecode(params) { return {}; }
        export function upstreamEncode(params) { return params.originalInput; }
        export function downstreamEncode(params) { return params.originalInput; }
    "#;
    let mut runtime = load(
        protocol,
        &[(
            "lib/lazy.js",
            b"globalThis.lazyEvaluations = (globalThis.lazyEvaluations ?? 0) + 1; export const value = 7;",
        )],
    );
    for _ in 0..2 {
        assert_eq!(
            runtime
                .upstream_decode(DecodeParams { input: "x".into() })
                .unwrap(),
            Document::parse_json(r#"{"value":7,"evaluations":1}"#).unwrap()
        );
    }
}

#[test]
fn nested_parent_imports_and_static_cycles_evaluate_each_module_once() {
    let protocol = r#"
        import { result } from "./lib/nested/bridge.js";
        export function upstreamDecode(params) { return result(); }
        export function downstreamDecode(params) { return {}; }
        export function upstreamEncode(params) { return params.originalInput; }
        export function downstreamEncode(params) { return params.originalInput; }
    "#;
    let extras: &[(&str, &[u8])] = &[
        (
            "lib/nested/bridge.js",
            br#"import { cycleValue } from "../cycle-a.js"; export function result() { return { value: cycleValue(), evaluations: globalThis.cycleEvaluations }; }"#,
        ),
        (
            "lib/cycle-a.js",
            br#"import { fromB } from "./cycle-b.js"; globalThis.cycleEvaluations = (globalThis.cycleEvaluations ?? 0) + 1; export const marker = 3; export function cycleValue() { return fromB(); }"#,
        ),
        (
            "lib/cycle-b.js",
            br#"import { marker } from "./cycle-a.js"; globalThis.cycleEvaluations = (globalThis.cycleEvaluations ?? 0) + 1; export function fromB() { return marker + 4; }"#,
        ),
    ];
    let mut runtime = load(protocol, extras);
    for _ in 0..2 {
        assert_eq!(
            runtime
                .upstream_decode(DecodeParams { input: "x".into() })
                .unwrap(),
            Document::parse_json(r#"{"value":7,"evaluations":2}"#).unwrap()
        );
    }
}

#[test]
fn relative_imports_cannot_escape_the_package_root() {
    let protocol = r#"
        import { value } from "../../escape.js";
        export function upstreamDecode(params) { return { value }; }
        export function downstreamDecode(params) { return {}; }
        export function upstreamEncode(params) { return params.originalInput; }
        export function downstreamEncode(params) { return params.originalInput; }
    "#;
    let package = read_package_zip(Cursor::new(archive(protocol, &[])), &Limits).unwrap();
    assert!(LocalSidecarRuntime::load(&package).is_err());
}
