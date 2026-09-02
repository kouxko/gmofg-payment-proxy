use serde_json::json;

use super::schema_snapshot;

#[test]
fn schema_snapshot_covers_required_unions_enums_and_nullable_fields() {
    let defs = &schema_snapshot()["tools"]["environment_candidate_create"]["inputSchema"]["$defs"];
    assert_eq!(defs["target"]["oneOf"].as_array().unwrap().len(), 2);
    assert_eq!(
        defs["listener"]["properties"]["data_plane"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(defs["documentValue"]["oneOf"].as_array().unwrap().len(), 6);
    assert_eq!(
        defs["documentMutation"]["oneOf"].as_array().unwrap().len(),
        4
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["burst_loss"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["upload_bytes_per_second"]["type"],
        json!(["integer", "null"])
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["download_bytes_per_second"]["type"],
        json!(["integer", "null"])
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["path_mtu"]["properties"]["mtu"]["type"],
        json!(["integer", "null"])
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["path_mtu"]["properties"]["mss_clamp"]["minimum"],
        1
    );
    assert_eq!(
        defs["workspace"]["required"],
        json!(["listeners", "rules", "android_network_profiles"])
    );
    assert!(defs["workspace"]["properties"]["http_rules"].is_null());
    assert!(defs["workspace"]["properties"]["protocol_rules"].is_null());
    assert_eq!(defs["ruleContent"]["oneOf"].as_array().unwrap().len(), 2);
    assert!(defs.get("conditionTree").is_none());
    for content in ["httpRuleContent", "socketRuleContent"] {
        assert_eq!(
            defs[content]["properties"]["condition"]["$ref"],
            "#/$defs/condition"
        );
        assert_eq!(
            defs[content]["properties"]["action"]["$ref"],
            "#/$defs/unifiedAction"
        );
        assert!(
            defs[content]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("condition"))
        );
        assert!(
            defs[content]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("action"))
        );
        assert!(defs[content]["properties"]["conditions"].is_null());
        assert!(defs[content]["properties"]["actions"].is_null());
    }
    assert_eq!(
        defs["rule"]["properties"]["stage"]["enum"],
        json!(["proxy_to_upstream", "proxy_to_app"])
    );
    assert!(
        !defs["rule"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("existing_rule_id"))
    );
}
