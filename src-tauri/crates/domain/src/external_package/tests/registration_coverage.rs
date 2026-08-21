use super::{registration, valid_registration_json};
use crate::{
    ExternalPackageDirection, ExternalPackageDirectionHooks, ExternalPackageMetadata,
    ExternalPackageMethodNamespace, ExternalPackageMethodSuffix, ExternalPackageRegistration,
    MAX_EXTERNAL_METHOD_SUFFIX_LEN, MAX_EXTERNAL_PACKAGE_NAME_LEN, ProtocolPackageId,
    ProtocolPackageRef, ProtocolPackageVersion,
};
use serde_json::json;

#[test]
fn registration_round_trip_preserves_the_strict_wire_contract() {
    let registration = registration();

    let encoded = serde_json::to_value(&registration).expect("registration serializes");
    let decoded: ExternalPackageRegistration =
        serde_json::from_value(encoded.clone()).expect("serialized registration remains valid");

    assert_eq!(decoded, registration);
    assert_eq!(encoded, valid_registration_json());
}

#[test]
fn registration_accessors_expose_both_directions_and_all_hook_roles() {
    let registration = registration();
    let upstream = registration.hooks().upstream();
    let downstream = registration.hooks().downstream();

    assert_eq!(upstream.frame().as_str(), "split_frame");
    assert_eq!(upstream.decode().as_str(), "decrypt_and_decode");
    assert_eq!(upstream.encode().as_str(), "encode_and_encrypt");
    assert_eq!(downstream.frame().as_str(), "split_frame");
    assert_eq!(downstream.decode().as_str(), "decrypt_and_decode");
    assert_eq!(downstream.encode().as_str(), "encode_and_encrypt");
    assert_eq!(
        registration.document().downstream().schema().fields().len(),
        2
    );
}

#[test]
fn metadata_constructor_preserves_identity_name_and_description() {
    let identity = ProtocolPackageRef {
        id: ProtocolPackageId::new("vendor-package").expect("valid package id"),
        version: ProtocolPackageVersion::new("2.3.4").expect("valid package version"),
    };

    let metadata = ExternalPackageMetadata::new(identity.clone(), "Vendor Package", "details")
        .expect("valid metadata");

    assert_eq!(metadata.identity(), &identity);
    assert_eq!(metadata.name(), "Vendor Package");
    assert_eq!(metadata.description(), "details");
}

#[test]
fn metadata_round_trip_preserves_the_strict_wire_contract() {
    let metadata = registration().package().clone();

    let encoded = serde_json::to_value(&metadata).expect("metadata serializes");
    let decoded: ExternalPackageMetadata =
        serde_json::from_value(encoded.clone()).expect("serialized metadata remains valid");

    assert_eq!(decoded, metadata);
    assert_eq!(
        encoded,
        json!({
            "id": "vendor-dukpt-iso8583",
            "name": "DUKPT ISO8583",
            "version": "1.0.0",
            "description": "使用外部密码设备处理 DUKPT 报文"
        })
    );
}

#[test]
fn metadata_deserialization_rejects_unknown_keys() {
    let value = json!({
        "id": "vendor-dukpt-iso8583",
        "name": "DUKPT ISO8583",
        "version": "1.0.0",
        "description": "details",
        "extra": true
    });

    assert!(serde_json::from_value::<ExternalPackageMetadata>(value).is_err());
}

#[test]
fn metadata_name_accepts_the_exact_unicode_limit() {
    let identity = ProtocolPackageRef {
        id: ProtocolPackageId::new("vendor-package").expect("valid package id"),
        version: ProtocolPackageVersion::new("1.0.0").expect("valid package version"),
    };
    let exact_limit = "界".repeat(MAX_EXTERNAL_PACKAGE_NAME_LEN);

    let metadata = ExternalPackageMetadata::new(identity, exact_limit.clone(), "")
        .expect("name at exact character limit is valid");

    assert_eq!(metadata.name(), exact_limit);
}

#[test]
fn metadata_name_rejects_more_than_the_unicode_limit() {
    let identity = ProtocolPackageRef {
        id: ProtocolPackageId::new("vendor-package").expect("valid package id"),
        version: ProtocolPackageVersion::new("1.0.0").expect("valid package version"),
    };

    let error =
        ExternalPackageMetadata::new(identity, "界".repeat(MAX_EXTERNAL_PACKAGE_NAME_LEN + 1), "")
            .expect_err("oversized name must fail");

    assert!(error.field_errors.contains_key("package.name"));
}

#[test]
fn method_suffix_accepts_an_identifier_at_the_exact_byte_limit() {
    let max_length = format!("_{}", "a".repeat(MAX_EXTERNAL_METHOD_SUFFIX_LEN - 1));
    let suffix = ExternalPackageMethodSuffix::try_from(max_length.clone())
        .expect("identifier at exact byte limit is valid");

    assert_eq!(suffix.as_str(), max_length);
}

#[test]
fn method_suffix_standard_conversions_preserve_the_validated_value() {
    let max_length = format!("_{}", "a".repeat(MAX_EXTERNAL_METHOD_SUFFIX_LEN - 1));
    let suffix = ExternalPackageMethodSuffix::new(max_length.clone())
        .expect("identifier at exact byte limit is valid");

    assert_eq!(suffix.to_string(), max_length);
    assert_eq!(String::from(suffix), max_length);
}

#[test]
fn method_suffix_rejects_identifiers_beyond_the_byte_limit() {
    let oversized = "a".repeat(MAX_EXTERNAL_METHOD_SUFFIX_LEN + 1);

    assert!(ExternalPackageMethodSuffix::new(oversized).is_err());
}

#[test]
fn method_namespaces_cover_every_direction_and_namespace() {
    let suffix = ExternalPackageMethodSuffix::new("render").expect("valid method suffix");

    assert_eq!(
        suffix.qualified(
            ExternalPackageMethodNamespace::Hooks,
            ExternalPackageDirection::Downstream,
        ),
        "hooks.downstream.render"
    );
    assert_eq!(
        suffix.qualified(
            ExternalPackageMethodNamespace::Document,
            ExternalPackageDirection::Upstream,
        ),
        "document.upstream.render"
    );
}

#[test]
fn hook_constructor_rejects_each_possible_duplicate_pair() {
    for (frame, decode, encode) in [
        ("same", "same", "encode"),
        ("same", "decode", "same"),
        ("frame", "same", "same"),
    ] {
        let result = ExternalPackageDirectionHooks::new(
            ExternalPackageMethodSuffix::new(frame).expect("valid frame suffix"),
            ExternalPackageMethodSuffix::new(decode).expect("valid decode suffix"),
            ExternalPackageMethodSuffix::new(encode).expect("valid encode suffix"),
        );

        assert!(result.is_err(), "duplicate tuple {frame}/{decode}/{encode}");
    }
}

#[test]
fn hook_wire_round_trip_preserves_all_methods() {
    let hooks = registration().hooks().upstream().clone();

    let encoded = serde_json::to_value(&hooks).expect("hooks serialize");
    let decoded: ExternalPackageDirectionHooks =
        serde_json::from_value(encoded.clone()).expect("serialized hooks remain valid");

    assert_eq!(decoded, hooks);
    assert_eq!(
        encoded,
        json!({
            "frame": "split_frame",
            "decode": "decrypt_and_decode",
            "encode": "encode_and_encrypt"
        })
    );
}
