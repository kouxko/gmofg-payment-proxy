#![cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]

mod codec;
mod display;
mod model;

use serde_json::Value;

#[allow(unsafe_code, clippy::same_length_and_capacity)]
mod component {
    use super::{
        decode_downstream, decode_upstream, display_downstream, display_upstream,
        encode_downstream, encode_upstream,
    };

    wit_bindgen::generate!({
        path: "../../../src-tauri/crates/package-runtime/wit",
        world: "http-package",
    });

    struct GmofgPaymentDll;

    impl Guest for GmofgPaymentDll {
        fn upstream_decode(input: String) -> Result<String, PackageError> {
            decode_upstream(&input).map_err(package_error("BODY_DECODE_FAILED"))
        }

        fn downstream_decode(input: String) -> Result<String, PackageError> {
            decode_downstream(&input).map_err(package_error("BODY_DECODE_FAILED"))
        }

        fn upstream_encode(
            original_input: String,
            document_json: String,
        ) -> Result<String, PackageError> {
            encode_upstream(&original_input, &document_json)
                .map_err(package_error("BODY_ENCODE_FAILED"))
        }

        fn downstream_encode(
            original_input: String,
            document_json: String,
        ) -> Result<String, PackageError> {
            encode_downstream(&original_input, &document_json)
                .map_err(package_error("BODY_ENCODE_FAILED"))
        }

        fn upstream_display(document_json: String) -> Result<String, PackageError> {
            display_upstream(&document_json).map_err(package_error("INTERNAL_ERROR"))
        }

        fn downstream_display(document_json: String) -> Result<String, PackageError> {
            display_downstream(&document_json).map_err(package_error("INTERNAL_ERROR"))
        }
    }

    fn package_error(code: &'static str) -> impl FnOnce(String) -> PackageError {
        move |message| PackageError {
            code: code.to_owned(),
            message,
        }
    }

    export!(GmofgPaymentDll);
}

const _: &str =
    include_str!("../../../../src-tauri/crates/package-runtime/wit/protocol-package.wit");

fn decode_upstream(input: &str) -> Result<String, String> {
    let document: Value = serde_json::from_str(input)
        .map_err(|error| format!("upstream HTTP body is not valid JSON: {error}"))?;
    serde_json::to_string(&document)
        .map_err(|error| format!("cannot serialize upstream passthrough Document: {error}"))
}

fn encode_upstream(original_input: &str, document_json: &str) -> Result<String, String> {
    let original: Value = serde_json::from_str(original_input)
        .map_err(|error| format!("original upstream HTTP body is not valid JSON: {error}"))?;
    let document: Value = serde_json::from_str(document_json)
        .map_err(|error| format!("upstream passthrough Document is not valid JSON: {error}"))?;
    if original != document {
        return Err("GMO-FG Payment DLL package does not allow upstream modification".to_owned());
    }
    Ok(original_input.to_owned())
}

fn display_upstream(document_json: &str) -> Result<String, String> {
    let document: Value = serde_json::from_str(document_json)
        .map_err(|error| format!("upstream passthrough Document is not valid JSON: {error}"))?;
    Ok(display::upstream(&document))
}

fn decode_downstream(input: &str) -> Result<String, String> {
    let document = codec::decode(input)?;
    serde_json::to_string(&document)
        .map_err(|error| format!("cannot serialize GMO-FG Payment DLL Document: {error}"))
}

fn encode_downstream(original_input: &str, document_json: &str) -> Result<String, String> {
    let original = codec::decode(original_input)?;
    let document: model::DllDocument = serde_json::from_str(document_json)
        .map_err(|error| format!("GMO-FG Payment DLL Document is not valid: {error}"))?;
    if original == document {
        return Ok(original_input.to_owned());
    }
    codec::encode(&document)
}

fn display_downstream(document_json: &str) -> Result<String, String> {
    let document: model::DllDocument = serde_json::from_str(document_json)
        .map_err(|error| format!("GMO-FG Payment DLL Document is not valid: {error}"))?;
    display::downstream(&document)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        decode_downstream, decode_upstream, display_downstream, encode_downstream, encode_upstream,
    };

    const D48: &str = include_str!("../tests/fixtures/d48.json");
    const CONNECTION_TEST: &str = include_str!("../tests/fixtures/connection-test.json");
    const CREDIT_SUCCESS: &str = include_str!("../tests/fixtures/credit-success.json");
    const UNION_PAY_SUCCESS: &str = include_str!("../tests/fixtures/union-pay-success.json");

    #[test]
    fn d48_error_response_decodes_displays_and_round_trips_exactly() {
        let document_json = decode_downstream(D48).expect("decode D48 response");
        let document: Value = serde_json::from_str(&document_json).expect("parse Document");

        assert_eq!(document["ErrorCode"], "D48");
        assert_eq!(document["ResponseID"], "A");
        assert_eq!(document["TransactionType"], "0001");
        assert_eq!(document["DebitInp"], "   ");
        assert_eq!(encode_downstream(D48, &document_json).unwrap(), D48);

        let html = display_downstream(&document_json).expect("display D48 response");
        assert!(html.contains("GMO-FG Payment DLL"));
        assert!(html.contains("<table"));
        assert!(html.contains("<thead>"));
        assert!(html.contains("<tbody>"));
        assert!(html.contains("D48"));
        assert!(!html.contains("<pre"));
    }

    #[test]
    fn connection_test_response_preserves_all_downstream_fields() {
        let document_json = decode_downstream(CONNECTION_TEST).expect("decode connection test");
        let document: Value = serde_json::from_str(&document_json).expect("parse Document");

        assert_eq!(document["TransactionType"], "0000");
        assert_eq!(document["TerminalID"], "1234567890123");
        assert_eq!(document["TerminalSeqNo"], "00001");
        assert_eq!(document["ResponseDateTime"], "20260903160000");
        assert_eq!(document["DLLUpdateAnn"], "1");
        assert_eq!(
            encode_downstream(CONNECTION_TEST, &document_json).unwrap(),
            CONNECTION_TEST
        );
    }

    #[test]
    fn complete_credit_response_parses_every_record_and_reserved_region() {
        let document_json = decode_downstream(CREDIT_SUCCESS).expect("decode complete Credit DLL");
        let document: Value = serde_json::from_str(&document_json).expect("parse Credit Document");

        let card_companies = document["KCCI_01"].as_array().unwrap();
        assert_eq!(card_companies.len(), 13);
        assert_eq!(
            card_companies
                .iter()
                .map(|company| company["card_ranges"].as_array().unwrap().len())
                .sum::<usize>(),
            62
        );
        assert_eq!(
            document["KJSI_01"]["reserved_79_89"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            10
        );
        assert_eq!(document["KCAK_01"].as_array().unwrap().len(), 6);
        assert_eq!(
            document["KCAK_01"]
                .as_array()
                .unwrap()
                .iter()
                .map(|group| group["public_keys"].as_array().unwrap().len())
                .sum::<usize>(),
            32
        );
        assert_eq!(document["KBRD_01"].as_array().unwrap().len(), 6);

        let companies = document["KICC_01"].as_array().unwrap();
        assert_eq!(companies.len(), 10);
        assert!(companies.iter().all(|company| {
            company["reserved_5_14"]
                .as_str()
                .is_some_and(|value| value.chars().count() == 9)
        }));
        let tables = companies
            .iter()
            .flat_map(|company| company["tables"].as_array().unwrap());
        let (risk_count, communication_count) = tables.fold((0, 0), |counts, table| {
            match table["table_id"].as_str().unwrap() {
                "0" => {
                    assert_eq!(
                        table["reserved_155_156"].as_str().unwrap().chars().count(),
                        1
                    );
                    assert_eq!(
                        table["reserved_351_372"].as_str().unwrap().chars().count(),
                        21
                    );
                    (counts.0 + 1, counts.1)
                }
                "9" => (counts.0, counts.1 + 1),
                id => panic!("unexpected table ID {id}"),
            }
        });
        assert_eq!((risk_count, communication_count), (25, 10));
        assert_eq!(
            encode_downstream(CREDIT_SUCCESS, &document_json).unwrap(),
            CREDIT_SUCCESS
        );
    }

    #[test]
    fn encode_rebuilds_credit_table_data_and_calculates_length_after_change() {
        let document_json = decode_downstream(CREDIT_SUCCESS).unwrap();
        let mut document: Value = serde_json::from_str(&document_json).unwrap();
        let ranges = document["KCCI_01"][0]["card_ranges"]
            .as_array_mut()
            .unwrap();
        let original_range_count = ranges.len();
        ranges.push(json!({
            "from": "1234567890123456",
            "to": "6543210987654321"
        }));

        let encoded = encode_downstream(CREDIT_SUCCESS, &document.to_string()).unwrap();
        let wire: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(wire["KCCI_01"]["Length"], "2510");
        assert_eq!(
            wire["KCCI_01"]["Data"].as_str().unwrap().chars().count(),
            2510
        );
        let decoded: Value = serde_json::from_str(&decode_downstream(&encoded).unwrap()).unwrap();
        assert_eq!(
            decoded["KCCI_01"][0]["card_ranges"]
                .as_array()
                .unwrap()
                .len(),
            original_range_count + 1
        );
    }

    #[test]
    fn encode_rebuilds_every_credit_table_from_document_fields() {
        let document_json = decode_downstream(CREDIT_SUCCESS).unwrap();
        let mut document: Value = serde_json::from_str(&document_json).unwrap();
        document["KJSI_01"]["phone_number_1"] = Value::String("11111111111111111111".to_owned());
        document["KBAT_01"]["sale_batch_process_function"] = Value::String("9".to_owned());
        document["KDST_01"]["port"] = Value::String("54321".to_owned());
        document["KCAK_01"][0]["public_keys"][0]["public_key_index"] =
            Value::String("99".to_owned());
        document["KBRD_01"][0]["validity"] = Value::String("9".to_owned());
        document["KICC_01"][0]["fallback"] = Value::String("9".to_owned());

        let mut changed_risk = false;
        let mut changed_communication = false;
        for company in document["KICC_01"].as_array_mut().unwrap() {
            for table in company["tables"].as_array_mut().unwrap() {
                match table["table_id"].as_str() {
                    Some("0") if !changed_risk => {
                        table["force_online"] = Value::String("9".to_owned());
                        changed_risk = true;
                    }
                    Some("9") if !changed_communication => {
                        table["communication_id"] = Value::String("ABCDEF".to_owned());
                        changed_communication = true;
                    }
                    _ => {}
                }
            }
        }
        assert!(changed_risk && changed_communication);

        let encoded = encode_downstream(CREDIT_SUCCESS, &document.to_string()).unwrap();
        let wire: Value = serde_json::from_str(&encoded).unwrap();
        for name in [
            "KCCI_01", "KJSI_01", "KBAT_01", "KDST_01", "KCAK_01", "KBRD_01", "KICC_01",
        ] {
            assert_eq!(
                wire[name]["Length"]
                    .as_str()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap(),
                wire[name]["Data"].as_str().unwrap().chars().count(),
                "{name} Length must be rebuilt from encoded Data"
            );
        }

        let decoded: Value = serde_json::from_str(&decode_downstream(&encoded).unwrap()).unwrap();
        assert_eq!(decoded["KJSI_01"]["phone_number_1"], "11111111111111111111");
        assert_eq!(decoded["KBAT_01"]["sale_batch_process_function"], "9");
        assert_eq!(decoded["KDST_01"]["port"], "54321");
        assert_eq!(
            decoded["KCAK_01"][0]["public_keys"][0]["public_key_index"],
            "99"
        );
        assert_eq!(decoded["KBRD_01"][0]["validity"], "9");
        assert_eq!(decoded["KICC_01"][0]["fallback"], "9");
        assert!(
            decoded["KICC_01"]
                .as_array()
                .unwrap()
                .iter()
                .any(|company| {
                    company["tables"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|table| table["table_id"] == "0" && table["force_online"] == "9")
                })
        );
        assert!(
            decoded["KICC_01"]
                .as_array()
                .unwrap()
                .iter()
                .any(|company| {
                    company["tables"].as_array().unwrap().iter().any(|table| {
                        table["table_id"] == "9" && table["communication_id"] == "ABCDEF"
                    })
                })
        );
    }

    #[test]
    fn union_pay_response_parses_a_table_and_reencodes_modified_fixed_width_field() {
        let document_json = decode_downstream(UNION_PAY_SUCCESS).expect("decode UnionPay DLL");
        let mut document: Value = serde_json::from_str(&document_json).unwrap();
        assert_eq!(document["KGIN_01"]["input_selection_info"], "001");
        assert_eq!(document["KGIN_01"]["merchant_code"], "611039210082840");
        assert_eq!(document["KGIN_01"]["bank_name"], "SMCCｷﾞﾝﾚﾝ           ");
        assert_eq!(
            encode_downstream(UNION_PAY_SUCCESS, &document_json).unwrap(),
            UNION_PAY_SUCCESS
        );

        document["KGIN_01"]["bank_name"] = Value::String("UPDATED BANK        ".to_owned());
        let encoded = encode_downstream(UNION_PAY_SUCCESS, &document.to_string()).unwrap();
        let wire: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(wire["KGIN_01"]["Length"], "57");
        let decoded: Value = serde_json::from_str(&decode_downstream(&encoded).unwrap()).unwrap();
        assert_eq!(decoded["KGIN_01"]["bank_name"], "UPDATED BANK        ");
    }

    #[test]
    fn decode_rejects_length_mismatch_unknown_ids_and_unknown_json_fields() {
        let mut bad_length: Value = serde_json::from_str(UNION_PAY_SUCCESS).unwrap();
        bad_length["KGIN_01"]["Length"] = Value::String("56".to_owned());
        assert!(
            decode_downstream(&bad_length.to_string())
                .unwrap_err()
                .contains("declares 56")
        );

        let mut unknown_id: Value = serde_json::from_str(CREDIT_SUCCESS).unwrap();
        let data = unknown_id["KICC_01"]["Data"].as_str().unwrap();
        let mut chars: Vec<_> = data.chars().collect();
        chars[14] = 'X';
        unknown_id["KICC_01"]["Data"] = Value::String(chars.into_iter().collect());
        assert!(
            decode_downstream(&unknown_id.to_string())
                .unwrap_err()
                .contains("unknown nested table ID X")
        );

        let mut unknown_field: Value = serde_json::from_str(D48).unwrap();
        unknown_field["UnknownTable_01"] = json!({"Length":"1","Data":"B"});
        assert!(decode_downstream(&unknown_field.to_string()).is_err());
    }

    #[test]
    fn decode_preserves_blank_risk_threshold_instead_of_dropping_the_record() {
        let mut wire: Value = serde_json::from_str(CREDIT_SUCCESS).unwrap();
        let data = wire["KICC_01"]["Data"].as_str().unwrap();
        let mut chars: Vec<_> = data.chars().collect();
        assert_eq!(chars[14], '9');
        assert_eq!(chars[25], '0');
        chars[45..51].fill(' ');
        let changed_data: String = chars.into_iter().collect();
        wire["KICC_01"]["Data"] = Value::String(changed_data);
        let changed_wire = wire.to_string();

        let document_json = decode_downstream(&changed_wire).unwrap();
        let document: Value = serde_json::from_str(&document_json).unwrap();
        let risk_tables: Vec<_> = document["KICC_01"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|company| company["tables"].as_array().unwrap())
            .filter(|table| table["table_id"] == "0")
            .collect();
        assert_eq!(risk_tables.len(), 25);
        assert!(
            risk_tables
                .iter()
                .any(|table| table["threshold"] == "      ")
        );
        assert_eq!(
            encode_downstream(&changed_wire, &document_json).unwrap(),
            changed_wire
        );
    }

    #[test]
    fn decode_rejects_ca_key_count_mismatch_and_wrong_outer_table_id() {
        let mut bad_count: Value = serde_json::from_str(CREDIT_SUCCESS).unwrap();
        let data = bad_count["KCAK_01"]["Data"].as_str().unwrap();
        let mut chars: Vec<_> = data.chars().collect();
        chars[3] = '0';
        bad_count["KCAK_01"]["Data"] = Value::String(chars.into_iter().collect());
        assert!(
            decode_downstream(&bad_count.to_string())
                .unwrap_err()
                .contains("declares 0 CA public keys")
        );

        let mut wrong_id: Value = serde_json::from_str(UNION_PAY_SUCCESS).unwrap();
        let data = wrong_id["KGIN_01"]["Data"].as_str().unwrap();
        wrong_id["KGIN_01"]["Data"] = Value::String(format!("B{}", &data[1..]));
        assert!(
            decode_downstream(&wrong_id.to_string())
                .unwrap_err()
                .contains("must be A")
        );
    }

    #[test]
    fn encode_rejects_invalid_width_and_transaction_table_mismatch() {
        let document_json = decode_downstream(UNION_PAY_SUCCESS).unwrap();
        let mut short_field: Value = serde_json::from_str(&document_json).unwrap();
        short_field["KGIN_01"]["bank_name"] = Value::String("SHORT".to_owned());
        assert!(
            encode_downstream(UNION_PAY_SUCCESS, &short_field.to_string())
                .unwrap_err()
                .contains("requires 20 characters")
        );

        let mut wrong_type: Value = serde_json::from_str(&document_json).unwrap();
        wrong_type["TransactionType"] = Value::String("0001".to_owned());
        assert!(
            encode_downstream(UNION_PAY_SUCCESS, &wrong_type.to_string())
                .unwrap_err()
                .contains("contains connection-test or UnionPay fields")
        );
    }

    #[test]
    fn display_escapes_modified_document_values() {
        use std::collections::BTreeSet;

        let document_json = decode_downstream(UNION_PAY_SUCCESS).unwrap();
        let mut document: Value = serde_json::from_str(&document_json).unwrap();
        document["KGIN_01"]["bank_name"] = Value::String("<script>&          ".to_owned());
        let root_field_count = document.as_object().unwrap().len();
        let union_pay_field_count = document["KGIN_01"].as_object().unwrap().len();
        let html = display_downstream(&document.to_string()).unwrap();
        assert!(html.contains("<table"));
        assert!(html.contains("<thead>"));
        assert!(html.contains("<tbody>"));
        assert!(html.contains("KGIN_01"));
        assert!(html.contains(&format!(
            "<summary><strong>基本信息</strong><span>Object · {root_field_count} fields</span></summary>"
        )));
        assert!(html.contains(&format!(
            "<summary><strong>KGIN_01</strong><span>Object · {union_pay_field_count} fields</span></summary>"
        )));
        assert_eq!(html.matches("<details open>").count(), 1);
        assert!(!html.contains(">$</caption>"));
        assert!(!html.contains(">$."));
        assert!(html.contains("<th>bank_name</th>"));
        assert!(html.contains("<th>reserved_79_89</th>"));
        assert!(html.contains("&lt;script&gt;&amp;"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<pre"));

        let table_tags: Vec<_> = html
            .match_indices("<table")
            .map(|(start, _)| {
                let end = html[start..].find('>').unwrap() + start + 1;
                &html[start..end]
            })
            .collect();
        assert!(table_tags.len() >= 4);
        assert!(table_tags.iter().all(|tag| {
            tag.contains("background-color:hsl(") && tag.contains("border-color:hsl(")
        }));
        assert_eq!(
            table_tags.iter().copied().collect::<BTreeSet<_>>().len(),
            table_tags.len(),
            "every table must receive a distinct deterministic color"
        );
    }

    #[test]
    fn display_preserves_the_complete_json_container_tree_as_disclosures() {
        fn container_count(value: &Value) -> usize {
            match value {
                Value::Object(object) => 1 + object.values().map(container_count).sum::<usize>(),
                Value::Array(array) => 1 + array.iter().map(container_count).sum::<usize>(),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 0,
            }
        }

        let document_json = decode_downstream(CREDIT_SUCCESS).unwrap();
        let document: Value = serde_json::from_str(&document_json).unwrap();
        let companies = document["KCCI_01"].as_array().unwrap();
        let first_ranges = companies[0]["card_ranges"].as_array().unwrap();
        let html = display_downstream(&document_json).unwrap();

        assert!(html.contains(&format!(
            "<summary><strong>KCCI_01</strong><span>Array · {} items</span></summary>",
            companies.len()
        )));
        assert!(
            html.contains("<summary><strong>[0]</strong><span>Object · 8 fields</span></summary>")
        );
        assert!(html.contains(&format!(
            "<summary><strong>card_ranges</strong><span>Array · {} items</span></summary>",
            first_ranges.len()
        )));
        assert!(
            html.contains("<summary><strong>[0]</strong><span>Object · 2 fields</span></summary>")
        );

        let expected_containers = container_count(&document);
        assert_eq!(html.matches("<details").count(), expected_containers);
        assert_eq!(html.matches("<summary>").count(), expected_containers);
        assert_eq!(html.matches("</summary>").count(), expected_containers);
        assert_eq!(html.matches("</details>").count(), expected_containers);
        assert_eq!(html.matches("<details open>").count(), 1);
    }

    #[test]
    fn upstream_abi_adapter_is_exact_read_only_json_passthrough() {
        let original = "{ \"RequestID\": \"R\", \"TransactionType\": \"0001\" }";
        let document = decode_upstream(original).unwrap();
        assert_eq!(encode_upstream(original, &document).unwrap(), original);

        let mut changed: Value = serde_json::from_str(&document).unwrap();
        changed["TransactionType"] = Value::String("0002".to_owned());
        assert!(
            encode_upstream(original, &changed.to_string())
                .unwrap_err()
                .contains("does not allow upstream modification")
        );
    }

    #[test]
    fn manifest_declares_only_the_complete_downstream_schema() {
        let manifest: Value = serde_json::from_str(include_str!("../manifest.json")).unwrap();
        assert_eq!(manifest["api"], 1);
        assert_eq!(manifest["kind"], "http");
        assert_eq!(manifest["package"]["id"], "gmofg-payment-dll");
        assert_eq!(manifest["document"]["upstream"], json!({}));
        let properties = manifest["document"]["downstream"]["schema"]["properties"]
            .as_object()
            .unwrap();
        for field in [
            "ErrorCode",
            "ResponseID",
            "TransactionType",
            "KCCI_01",
            "KJSI_01",
            "KCAK_01",
            "KICC_01",
            "KGIN_01",
            "GCAK_01",
            "GICC_01",
        ] {
            assert!(
                properties.contains_key(field),
                "missing schema field {field}"
            );
        }
    }
}
