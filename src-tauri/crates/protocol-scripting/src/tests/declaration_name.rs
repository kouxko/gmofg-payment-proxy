use crate::{
    MAX_PACKAGE_FILE_PATH_BYTES, MAX_PROTOCOL_FUNCTION_NAME_BYTES, PackageFilePath,
    ProtocolFunctionName, ProtocolPackageParseErrorCode,
};

#[test]
fn package_paths_accept_relative_utf8_and_exact_maximum() {
    let unicode = PackageFilePath::new("脚本/协议.rhai").unwrap();
    assert_eq!(unicode.as_str(), "脚本/协议.rhai");
    assert_eq!(unicode.to_string(), "脚本/协议.rhai");

    let maximum = format!("a/{}", "b".repeat(MAX_PACKAGE_FILE_PATH_BYTES - 2));
    assert_eq!(maximum.len(), MAX_PACKAGE_FILE_PATH_BYTES);
    assert!(PackageFilePath::new(maximum).is_ok());
}

#[test]
fn package_paths_reject_every_unsafe_shape() {
    let cases = [
        "",
        "/absolute.rhai",
        "../escape.rhai",
        "scripts/../escape.rhai",
        "./script.rhai",
        "scripts/./main.rhai",
        "scripts//main.rhai",
        "C:/main.rhai",
        "scripts\\main.rhai",
        "scripts/\nmain.rhai",
    ];
    for value in cases {
        let error = PackageFilePath::new(value).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::ManifestInvalid);
    }
    assert!(PackageFilePath::new("x".repeat(MAX_PACKAGE_FILE_PATH_BYTES + 1)).is_err());
}

#[test]
fn function_names_cover_valid_boundaries_and_invalid_identifiers() {
    for valid in ["frame", "_decode", "EncodeV1", "a0"] {
        let function = ProtocolFunctionName::new(valid).unwrap();
        assert_eq!(function.as_str(), valid);
        assert_eq!(function.to_string(), valid);
    }
    let maximum = format!("f{}", "x".repeat(MAX_PROTOCOL_FUNCTION_NAME_BYTES - 1));
    assert!(ProtocolFunctionName::new(maximum).is_ok());

    for invalid in [
        "",
        "1frame",
        "decode-frame",
        "decode.frame",
        "金额",
        "while",
        "Fn",
    ] {
        let error = ProtocolFunctionName::new(invalid).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::ManifestInvalid);
    }
    assert!(
        ProtocolFunctionName::new(format!("f{}", "x".repeat(MAX_PROTOCOL_FUNCTION_NAME_BYTES)))
            .is_err()
    );
}
