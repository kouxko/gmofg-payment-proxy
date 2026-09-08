use super::*;
use crate::ErrorCode;

#[test]
fn document_number_retains_the_existing_negative_zero_representation() {
    for input in ["-0", "-0.0"] {
        let document = Document::parse_json(input).unwrap();
        assert_eq!(document.to_json().unwrap(), "-0.0");
        let number: DocumentNumber = serde_json::from_str(input).unwrap();
        assert!(number.get().is_sign_negative());
        assert_eq!(serde_json::to_string(&number).unwrap(), "-0.0");
        assert_eq!(
            serde_json::to_value(number)
                .unwrap()
                .as_f64()
                .unwrap()
                .to_bits(),
            (-0.0_f64).to_bits()
        );
    }
}

#[test]
fn document_integral_numbers_serialize_as_integer_tokens_at_every_json_boundary() {
    for (input, expected) in [
        ("0", "0"),
        ("0.0", "0"),
        ("1", "1"),
        ("2.0", "2"),
        ("-2.0", "-2"),
        ("100.00", "100"),
        ("1e2", "100"),
        ("9007199254740991", "9007199254740991"),
        ("-9007199254740991", "-9007199254740991"),
    ] {
        let parsed = Document::parse_json(input).unwrap();
        let deserialized: Document = serde_json::from_str(input).unwrap();
        let number: DocumentNumber = serde_json::from_str(input).unwrap();
        let constructed = Document::new(DocumentValue::Number(
            DocumentNumber::new(number.get()).unwrap(),
        ));
        for document in [&parsed, &deserialized, &constructed] {
            assert_eq!(document.to_json().unwrap(), expected, "{input}");
            assert_eq!(
                serde_json::to_string(document).unwrap(),
                expected,
                "{input}"
            );
            let value = serde_json::to_value(document).unwrap();
            assert!(value.is_i64(), "{input}: {value:?}");
            assert_eq!(value.to_string(), expected, "{input}");
            assert_eq!(
                serde_json::from_value::<Document>(value).unwrap(),
                *document
            );
        }
        assert_eq!(serde_json::to_string(&number).unwrap(), expected, "{input}");
    }
}

#[test]
fn document_fractional_numbers_keep_their_values_without_rounding() {
    for value in [
        2.5,
        -2.5,
        0.1,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::from_bits(1.0_f64.to_bits() - 1),
        f64::from_bits(1.0_f64.to_bits() + 1),
        4_503_599_627_370_495.5,
    ] {
        let number = DocumentNumber::new(value).unwrap();
        let serialized = serde_json::to_value(number).unwrap();
        assert!(serialized.is_f64(), "{value}: {serialized:?}");
        assert_eq!(serialized.as_f64().unwrap().to_bits(), value.to_bits());
        assert_eq!(
            serde_json::to_string(&number).unwrap(),
            serde_json::to_string(&value).unwrap(),
            "fractional formatting remains unchanged"
        );
    }
}

#[test]
fn document_nested_numbers_normalize_without_changing_strings_or_other_types() {
    let document = Document::parse_json(
        r#"{"array":[0,1,2.0,2.5,"2.0",true,null],"object":{"count":2,"decimal":-2.5}}"#,
    )
    .unwrap();
    let expected = r#"{"array":[0,1,2,2.5,"2.0",true,null],"object":{"count":2,"decimal":-2.5}}"#;
    assert_eq!(document.to_json().unwrap(), expected);
    assert_eq!(serde_json::to_string(&document).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(&document).unwrap(),
        serde_json::from_str::<serde_json::Value>(expected).unwrap()
    );

    let bytes = DocumentValue::byte_array([0, 1, 255]);
    assert_eq!(serde_json::to_string(&bytes).unwrap(), "[0,1,255]");
    let integer = DocumentValue::integer(-2).unwrap();
    assert_eq!(serde_json::to_string(&integer).unwrap(), "-2");
}

#[test]
fn document_number_serialization_does_not_relax_invalid_number_rejection() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            DocumentNumber::new(value).unwrap_err().code,
            ErrorCode::DocumentNumberInvalid
        );
    }
    for input in [
        "9007199254740992",
        "-9007199254740992",
        "9007199254740992.0",
        "1e20",
    ] {
        assert_eq!(
            Document::parse_json(input).unwrap_err().code,
            ErrorCode::DocumentUnsafeInteger
        );
        assert!(serde_json::from_str::<DocumentNumber>(input).is_err());
        assert!(serde_json::from_str::<Document>(input).is_err());
    }
}
