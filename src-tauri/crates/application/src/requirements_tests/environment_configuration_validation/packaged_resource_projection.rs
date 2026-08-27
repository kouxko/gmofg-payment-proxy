use super::*;

use crate::{
    environment_configuration::EnvironmentProjectedCandidate,
    requirements_tests::test_environment_identity_allocator,
};

#[test]
fn packaged_resource_candidate_projects_with_builtin_package_and_no_private_materials() {
    let mut value: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    value["target"] = serde_json::json!({
        "mode":"new",
        "name":"G032 Packaged Full Resources",
    });
    value["workspace"]["listeners"] = serde_json::json!([
        value["workspace"]["listeners"][0].clone(),
        value["workspace"]["listeners"][1].clone(),
    ]);
    let http = &mut value["workspace"]["listeners"][0];
    http["enabled"] = serde_json::json!(false);
    http["bind_address"] = serde_json::json!("127.0.0.1");
    http["data_plane"]["settings"]["authentication"] = serde_json::json!({"mode":"none"});
    http["data_plane"]["settings"]["mitm"] = serde_json::json!({
        "enabled":false,
        "authority_allowlist":[],
        "root_ca_selector":null,
        "maximum_cached_leaf_certificates":256,
    });
    http["data_plane"]["settings"]["downstream_tls"] = serde_json::json!({
        "enabled":false,
        "server_identity_alias":null,
        "dynamic_sni_allowlist":[],
        "client_authentication":{"mode":"disabled"},
    });
    http["data_plane"]["settings"]["body_processing"] = serde_json::json!({
        "mode":"protocol",
        "package":{"id":"iso8583-ascii-standard","version":"1.0.0"},
    });
    http["data_plane"]["settings"]["fixed_server"] = serde_json::Value::Null;

    let socket = &mut value["workspace"]["listeners"][1];
    socket["enabled"] = serde_json::json!(false);
    socket["data_plane"]["settings"]["topology"] = serde_json::json!({
        "mode":"local_responder",
        "settings":{"downstream_security":{"mode":"tcp"}},
    });
    socket["data_plane"]["settings"]["processing"] = serde_json::json!({
        "mode":"scripted",
        "settings":{"package":{"id":"iso8583-ascii-standard","version":"1.0.0"}},
    });
    value["workspace"]["protocol_rules"][0]["package"] =
        serde_json::json!({"id":"iso8583-ascii-standard","version":"1.0.0"});
    value["materials"] = serde_json::json!({"certificates":[],"secrets":[]});

    let candidate =
        crate::parse_environment_configuration_candidate_v1(&serde_json::to_vec(&value).unwrap())
            .unwrap();
    let allocator = test_environment_identity_allocator();
    let projected = EnvironmentProjectedCandidate::project(candidate, None, allocator.port())
        .unwrap_or_else(|error| panic!("packaged resource projection failed: {error:?}"));

    assert_eq!(projected.workspace().listeners.len(), 2);
    assert_eq!(projected.workspace().rules.len(), 14);
    assert_eq!(projected.workspace().protocol_rules.len(), 1);
    assert_eq!(projected.workspace().android_network_profiles.len(), 1);
}
