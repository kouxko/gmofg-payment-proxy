use intercept_proxy_domain::{ProtocolRuleStage, RuleStage};

#[test]
fn legacy_message_stages_fail_closed_at_the_domain_wire_boundary() {
    for legacy in ["app_to_proxy", "upstream_to_proxy"] {
        assert!(serde_json::from_str::<RuleStage>(&format!("\"{legacy}\"")).is_err());
        assert!(serde_json::from_str::<ProtocolRuleStage>(&format!("\"{legacy}\"")).is_err());
    }
}

#[test]
fn authoritative_write_stages_remain_deserializable() {
    assert_eq!(
        serde_json::from_str::<RuleStage>("\"proxy_to_upstream\"").unwrap(),
        RuleStage::ProxyToUpstream,
    );
    assert_eq!(
        serde_json::from_str::<ProtocolRuleStage>("\"proxy_to_app\"").unwrap(),
        ProtocolRuleStage::ProxyToApp,
    );
}
