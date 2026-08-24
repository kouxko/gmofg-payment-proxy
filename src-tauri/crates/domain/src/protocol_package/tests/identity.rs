use super::*;
use crate::ErrorCode;

#[test]
fn package_id_accepts_contract_and_boundaries() {
    for value in ["a", "iso8583", "iso-8583", "a-"] {
        let id = ProtocolPackageId::new(value).unwrap();
        assert_eq!(id.as_str(), value);
        assert_eq!(id.to_string(), value);
        assert_eq!(value.parse::<ProtocolPackageId>().unwrap(), id);
    }

    let maximum = format!("a{}", "-".repeat(MAX_PROTOCOL_PACKAGE_ID_LEN - 1));
    assert_eq!(ProtocolPackageId::new(&maximum).unwrap().as_str(), maximum);
}

#[test]
fn package_id_rejects_every_invalid_character_class() {
    for value in [
        "", "1iso", "Iso", "iso_8583", "iso.8583", "iso/8583", "iso 8583", "协议",
    ] {
        let error = ProtocolPackageId::new(value).unwrap_err();
        assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid, "{value}");
        assert!(error.field_errors.contains_key("package.id"));
    }
    let too_long = format!("a{}", "a".repeat(MAX_PROTOCOL_PACKAGE_ID_LEN));
    assert!(ProtocolPackageId::new(too_long).is_err());
}

#[test]
fn package_id_deserialization_cannot_bypass_validation() {
    let id: ProtocolPackageId = serde_json::from_str("\"iso-8583\"").unwrap();
    assert_eq!(serde_json::to_string(&id).unwrap(), "\"iso-8583\"");
    assert!(serde_json::from_str::<ProtocolPackageId>("\"BAD\"").is_err());
}

#[test]
fn package_version_accepts_complete_semver_and_boundaries() {
    for value in ["0.0.0", "1.2.3", "1.2.3-beta.1", "1.2.3+build.7"] {
        let version = ProtocolPackageVersion::new(value).unwrap();
        assert_eq!(version.as_str(), value);
        assert_eq!(version.to_string(), value);
        assert_eq!(value.parse::<ProtocolPackageVersion>().unwrap(), version);
    }

    let maximum = format!("1.0.0+{}", "a".repeat(MAX_PROTOCOL_PACKAGE_VERSION_LEN - 6));
    assert_eq!(maximum.len(), MAX_PROTOCOL_PACKAGE_VERSION_LEN);
    assert!(ProtocolPackageVersion::new(maximum).is_ok());
}

#[test]
fn package_version_exposes_semver_numeric_order_for_grouped_views() {
    let two = ProtocolPackageVersion::new("2.0.0").unwrap();
    let ten = ProtocolPackageVersion::new("10.0.0").unwrap();

    assert_eq!(two.semantic_cmp(&ten), std::cmp::Ordering::Less);
    assert_eq!(ten.semantic_cmp(&two), std::cmp::Ordering::Greater);
}

#[test]
fn package_version_rejects_invalid_truncated_unicode_and_too_long_values() {
    for value in ["", "1", "1.2", "v1.2.3", "01.2.3", "1.2.3-β"] {
        let error = ProtocolPackageVersion::new(value).unwrap_err();
        assert_eq!(error.code, ErrorCode::ProtocolPackageInvalid, "{value}");
        assert!(error.field_errors.contains_key("package.version"));
    }
    let too_long = format!("1.0.0+{}", "a".repeat(MAX_PROTOCOL_PACKAGE_VERSION_LEN - 5));
    assert_eq!(too_long.len(), MAX_PROTOCOL_PACKAGE_VERSION_LEN + 1);
    assert!(ProtocolPackageVersion::new(too_long).is_err());
    assert!(serde_json::from_str::<ProtocolPackageVersion>("\"1.2\"").is_err());
}

#[test]
fn package_reference_clone_eq_and_serde_round_trip() {
    let reference = ProtocolPackageRef {
        id: ProtocolPackageId::new("iso-8583").unwrap(),
        version: ProtocolPackageVersion::new("1.2.3").unwrap(),
    };
    assert_eq!(reference.clone(), reference);
    let json = serde_json::to_string(&reference).unwrap();
    assert_eq!(
        serde_json::from_str::<ProtocolPackageRef>(&json).unwrap(),
        reference
    );
    for forbidden in ["extra", "digest", "content_sha256", "signature"] {
        let value = serde_json::json!({
            "id": "iso-8583",
            "version": "1.2.3",
            (forbidden): true,
        });
        assert!(
            serde_json::from_value::<ProtocolPackageRef>(value).is_err(),
            "forbidden package identity field {forbidden} must be rejected"
        );
    }
}
