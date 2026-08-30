use std::io::{Cursor, Write};

use intercept_proxy_domain::Document;
use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult,
};
use intercept_proxy_package_runtime::{
    LocalSidecarRuntime, PackageArchiveResourceLimits, read_package_zip,
};
use zip::{ZipWriter, write::SimpleFileOptions};

const SOCKET_MANIFEST: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/socket-manifest.json"
);
const HTTP_MANIFEST: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);

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

fn package_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut output);
        for (path, bytes) in entries {
            writer
                .start_file(*path, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    output.into_inner()
}

fn runtime(
    manifest: &str,
    protocol: &str,
    display: &str,
    extra: &[(&str, &[u8])],
) -> LocalSidecarRuntime {
    let mut entries = vec![
        ("manifest.json", manifest.as_bytes()),
        ("protocol.js", protocol.as_bytes()),
        ("display.js", display.as_bytes()),
    ];
    entries.extend_from_slice(extra);
    let archive = read_package_zip(Cursor::new(package_zip(&entries)), &Limits).unwrap();
    LocalSidecarRuntime::load(&archive).unwrap()
}

#[test]
fn required_exports_are_prechecked_without_calling_package_code() {
    let protocol = r#"
        let calls = 0;
        export function upstreamFrame(params) { calls += 1; return { status: "need_more" }; }
        export function downstreamFrame(params) { return { status: "need_more" }; }
        export function upstreamDecode(params) {
            const result = { calls };
            calls += 1;
            return result;
        }
        export function downstreamDecode(params) { return { ok: true }; }
        export function upstreamEncode(params) { return params.originalInput; }
        export function downstreamEncode(params) { return params.originalInput; }
    "#;
    let display = r#"
        export function upstreamDisplay(params) { return "<p>up</p>"; }
        export function downstreamDisplay(params) { return "<p>down</p>"; }
    "#;
    let mut runtime = runtime(SOCKET_MANIFEST, protocol, display, &[]);
    assert_eq!(
        runtime
            .upstream_decode(DecodeParams {
                input: CanonicalBase64::from_bytes(b"input").as_str().to_owned(),
            })
            .unwrap(),
        Document::parse_json(r#"{"calls":0}"#).unwrap(),
    );
}

#[test]
fn missing_or_non_callable_fixed_exports_fail_before_registration() {
    let display = r#"
        export function upstreamDisplay(params) { return ""; }
        export function downstreamDisplay(params) { return ""; }
    "#;
    let missing = read_package_zip(
        Cursor::new(package_zip(&[
            ("manifest.json", SOCKET_MANIFEST.as_bytes()),
            ("protocol.js", b"export function upstreamFrame() {}"),
            ("display.js", display.as_bytes()),
        ])),
        &Limits,
    )
    .unwrap();
    assert!(LocalSidecarRuntime::load(&missing).is_err());

    let non_callable = read_package_zip(
        Cursor::new(package_zip(&[
            ("manifest.json", SOCKET_MANIFEST.as_bytes()),
            (
                "protocol.js",
                br"
                    export const upstreamFrame = 1;
                    export function downstreamFrame() {}
                    export function upstreamDecode() {}
                    export function downstreamDecode() {}
                    export function upstreamEncode() {}
                    export function downstreamEncode() {}
                ",
            ),
            ("display.js", display.as_bytes()),
        ])),
        &Limits,
    )
    .unwrap();
    assert!(LocalSidecarRuntime::load(&non_callable).is_err());
}

#[test]
fn relative_esm_modules_are_evaluated_once_and_exports_are_cached() {
    let protocol = r#"
        import { next } from "./lib/state.js";
        export function upstreamFrame(params) { return { status: "need_more", requiredBytes: next() }; }
        export function downstreamFrame(params) { return { status: "need_more", requiredBytes: next() }; }
        export function upstreamDecode(params) { return { count: next() }; }
        export function downstreamDecode(params) { return { count: next() }; }
        export function upstreamEncode(params) { return String(next()); }
        export function downstreamEncode(params) { return String(next()); }
    "#;
    let display = r#"
        export function upstreamDisplay(params) { return "<p>" + params.document.count + "</p>"; }
        export function downstreamDisplay(params) { return "<p>" + params.document.count + "</p>"; }
    "#;
    let mut runtime = runtime(
        SOCKET_MANIFEST,
        protocol,
        display,
        &[(
            "lib/state.js",
            b"let value = 0; export function next() { value += 1; return value; }",
        )],
    );
    assert_eq!(
        runtime
            .upstream_frame(FrameParams {
                buffer: CanonicalBase64::from_bytes(b"abc"),
            })
            .unwrap(),
        FrameResult::NeedMore {
            required_bytes: Some(1)
        }
    );
    assert_eq!(
        runtime
            .downstream_decode(DecodeParams {
                input: "AA==".into()
            })
            .unwrap(),
        Document::parse_json(r#"{"count":2}"#).unwrap()
    );
}

#[test]
fn fixed_exports_are_cached_after_registration_precheck() {
    let protocol = r#"
        let decode = () => ({ implementation: "initial" });
        export { decode as upstreamDecode };
        export function upstreamFrame(params) {
            decode = () => ({ implementation: "replacement" });
            return { status: "need_more" };
        }
        export function downstreamFrame(params) { return { status: "need_more" }; }
        export function downstreamDecode(params) { return {}; }
        export function upstreamEncode(params) { return new Uint8Array([]); }
        export function downstreamEncode(params) { return new Uint8Array([]); }
    "#;
    let display = r#"
        export function upstreamDisplay(params) { return ""; }
        export function downstreamDisplay(params) { return ""; }
    "#;
    let mut runtime = runtime(SOCKET_MANIFEST, protocol, display, &[]);
    runtime
        .upstream_frame(FrameParams {
            buffer: CanonicalBase64::from_bytes(b"x"),
        })
        .unwrap();
    assert_eq!(
        runtime
            .upstream_decode(DecodeParams {
                input: CanonicalBase64::from_bytes(b"x").as_str().to_owned(),
            })
            .unwrap(),
        Document::parse_json(r#"{"implementation":"initial"}"#).unwrap(),
    );
}

#[test]
fn only_package_relative_esm_specifiers_are_accepted() {
    let protocol = r#"
        import { value } from "lib/value.js";
        export function upstreamFrame(params) { return { status: "need_more", requiredBytes: value }; }
        export function downstreamFrame(params) { return { status: "need_more" }; }
        export function upstreamDecode(params) { return {}; }
        export function downstreamDecode(params) { return {}; }
        export function upstreamEncode(params) { return new Uint8Array([]); }
        export function downstreamEncode(params) { return new Uint8Array([]); }
    "#;
    let display = r#"
        export function upstreamDisplay(params) { return ""; }
        export function downstreamDisplay(params) { return ""; }
    "#;
    let archive = read_package_zip(
        Cursor::new(package_zip(&[
            ("manifest.json", SOCKET_MANIFEST.as_bytes()),
            ("protocol.js", protocol.as_bytes()),
            ("display.js", display.as_bytes()),
            ("lib/value.js", b"export const value = 1;"),
        ])),
        &Limits,
    )
    .unwrap();
    assert!(LocalSidecarRuntime::load(&archive).is_err());
}

#[test]
fn http_hooks_receive_and_return_unicode_strings_without_socket_frame_exports() {
    let protocol = r#"
        export function upstreamDecode(params) {
            return { input: params.input, inputType: typeof params.input };
        }
        export function downstreamDecode(params) { return {}; }
        export function upstreamEncode(params) { return params.originalInput + ":" + params.document.input; }
        export function downstreamEncode(params) { return params.originalInput; }
    "#;
    let display = r#"
        export function upstreamDisplay(params) { return "<p>" + params.document.input + "</p>"; }
        export function downstreamDisplay(params) { return ""; }
    "#;
    let mut runtime = runtime(HTTP_MANIFEST, protocol, display, &[]);
    let document = runtime
        .upstream_decode(DecodeParams {
            input: "こんにちは".into(),
        })
        .unwrap();
    assert_eq!(
        document,
        Document::parse_json(r#"{"input":"こんにちは","inputType":"string"}"#).unwrap(),
    );
    assert_eq!(
        runtime
            .upstream_encode(EncodeParams {
                original_input: "本文".into(),
                document,
            })
            .unwrap(),
        "本文:こんにちは",
    );
}

#[test]
fn socket_base64_is_presented_to_javascript_as_uint8array_and_returns_canonical_base64() {
    let protocol = r#"
        export function upstreamFrame(params) {
            if (!(params.buffer instanceof Uint8Array)) throw new Error("not bytes");
            return { status: "complete", consumedBytes: params.buffer.length };
        }
        export function downstreamFrame(params) { return { status: "need_more" }; }
        export function upstreamDecode(params) {
            if (!(params.input instanceof Uint8Array)) throw new Error("not bytes");
            return { first: params.input[0], length: params.input.length };
        }
        export function downstreamDecode(params) { return {}; }
        export function upstreamEncode(params) { return new Uint8Array([params.document.first, 90]); }
        export function downstreamEncode(params) { return new Uint8Array([]); }
    "#;
    let display = r#"
        export function upstreamDisplay(params) { return "<p>up</p>"; }
        export function downstreamDisplay(params) { return "<p>down</p>"; }
    "#;
    let mut runtime = runtime(SOCKET_MANIFEST, protocol, display, &[]);
    assert_eq!(
        runtime
            .upstream_frame(FrameParams {
                buffer: CanonicalBase64::from_bytes(&[65, 66]),
            })
            .unwrap(),
        FrameResult::complete(2).unwrap()
    );
    let document = runtime
        .upstream_decode(DecodeParams {
            input: CanonicalBase64::from_bytes(&[65, 66]).as_str().to_owned(),
        })
        .unwrap();
    assert_eq!(
        document,
        Document::parse_json(r#"{"first":65,"length":2}"#).unwrap()
    );
    assert_eq!(
        runtime
            .upstream_encode(EncodeParams {
                original_input: "QQ==".into(),
                document,
            })
            .unwrap(),
        "QVo="
    );
}

#[test]
fn socket_encode_rejects_non_uint8array_results() {
    let protocol = r#"
        export function upstreamFrame(params) { return { status: "need_more" }; }
        export function downstreamFrame(params) { return { status: "need_more" }; }
        export function upstreamDecode(params) { return {}; }
        export function downstreamDecode(params) { return {}; }
        export function upstreamEncode(params) { return [1, 2]; }
        export function downstreamEncode(params) { return new Uint8Array([]); }
    "#;
    let display = r#"
        export function upstreamDisplay(params) { return ""; }
        export function downstreamDisplay(params) { return ""; }
    "#;
    let mut runtime = runtime(SOCKET_MANIFEST, protocol, display, &[]);
    assert!(
        runtime
            .upstream_encode(EncodeParams {
                original_input: CanonicalBase64::from_bytes(&[]).as_str().to_owned(),
                document: Document::parse_json("{}").unwrap(),
            })
            .is_err()
    );
}

#[test]
fn all_eight_fixed_exports_map_to_the_matching_direction() {
    let protocol = r#"
        export function upstreamFrame(params) { return { status: "need_more", requiredBytes: 1 }; }
        export function downstreamFrame(params) { return { status: "need_more", requiredBytes: 2 }; }
        export function upstreamDecode(params) { return { direction: "upstream" }; }
        export function downstreamDecode(params) { return { direction: "downstream" }; }
        export function upstreamEncode(params) { return new Uint8Array([1]); }
        export function downstreamEncode(params) { return new Uint8Array([2]); }
    "#;
    let display = r#"
        export function upstreamDisplay(params) { return "upstream"; }
        export function downstreamDisplay(params) { return "downstream"; }
    "#;
    let mut runtime = runtime(SOCKET_MANIFEST, protocol, display, &[]);
    let frame = FrameParams {
        buffer: CanonicalBase64::from_bytes(b"ab"),
    };
    assert_eq!(
        runtime.upstream_frame(frame.clone()).unwrap(),
        FrameResult::NeedMore {
            required_bytes: Some(1),
        }
    );
    assert_eq!(
        runtime.downstream_frame(frame).unwrap(),
        FrameResult::NeedMore {
            required_bytes: Some(2),
        }
    );
    let input = DecodeParams {
        input: CanonicalBase64::from_bytes(b"x").as_str().to_owned(),
    };
    let upstream = runtime.upstream_decode(input.clone()).unwrap();
    let downstream = runtime.downstream_decode(input).unwrap();
    assert_eq!(
        upstream,
        Document::parse_json(r#"{"direction":"upstream"}"#).unwrap()
    );
    assert_eq!(
        downstream,
        Document::parse_json(r#"{"direction":"downstream"}"#).unwrap()
    );
    assert_eq!(
        runtime
            .upstream_encode(EncodeParams {
                original_input: CanonicalBase64::from_bytes(b"x").as_str().to_owned(),
                document: upstream.clone(),
            })
            .unwrap(),
        "AQ=="
    );
    assert_eq!(
        runtime
            .downstream_encode(EncodeParams {
                original_input: CanonicalBase64::from_bytes(b"x").as_str().to_owned(),
                document: downstream.clone(),
            })
            .unwrap(),
        "Ag=="
    );
    assert_eq!(
        runtime
            .upstream_display(DisplayParams { document: upstream })
            .unwrap(),
        "upstream"
    );
    assert_eq!(
        runtime
            .downstream_display(DisplayParams {
                document: downstream,
            })
            .unwrap(),
        "downstream"
    );
}
