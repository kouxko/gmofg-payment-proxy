use std::collections::{BTreeMap, BTreeSet};

use intercept_proxy_domain::{CertificateReferenceId, ListenerId, ProxyListener, ProxyWorkspace};

pub(super) fn affected_listener_ids(
    persisted: Option<&ProxyWorkspace>,
    candidate: &ProxyWorkspace,
) -> Vec<ListenerId> {
    let Some(persisted) = persisted else {
        return candidate
            .listeners
            .iter()
            .map(|listener| listener.id)
            .collect();
    };

    let mut affected = direct_listener_changes(persisted, candidate);
    lift_rule_changes(persisted, candidate, &mut affected);
    lift_material_changes(persisted, candidate, &mut affected);
    lift_android_route_changes(persisted, candidate, &mut affected);
    affected.into_iter().collect()
}

fn direct_listener_changes(
    persisted: &ProxyWorkspace,
    candidate: &ProxyWorkspace,
) -> BTreeSet<ListenerId> {
    persisted
        .listeners
        .iter()
        .chain(&candidate.listeners)
        .filter(|listener| {
            persisted
                .listeners
                .iter()
                .find(|current| current.id == listener.id)
                != candidate
                    .listeners
                    .iter()
                    .find(|current| current.id == listener.id)
        })
        .map(|listener| listener.id)
        .collect()
}

fn lift_rule_changes(
    persisted: &ProxyWorkspace,
    candidate: &ProxyWorkspace,
    affected: &mut BTreeSet<ListenerId>,
) {
    if persisted.rule_definitions != candidate.rule_definitions {
        affected.extend(
            persisted
                .rule_definitions
                .iter()
                .chain(&candidate.rule_definitions)
                .map(intercept_proxy_domain::RuleDefinition::listener_id),
        );
    }
}

fn lift_material_changes(
    persisted: &ProxyWorkspace,
    candidate: &ProxyWorkspace,
    affected: &mut BTreeSet<ListenerId>,
) {
    let references = persisted
        .certificate_references
        .iter()
        .chain(&candidate.certificate_references)
        .filter(|reference| {
            persisted
                .certificate_references
                .iter()
                .find(|current| current.id == reference.id)
                != candidate
                    .certificate_references
                    .iter()
                    .find(|current| current.id == reference.id)
        })
        .map(|reference| reference.id)
        .collect::<BTreeSet<_>>();
    if references.is_empty() {
        return;
    }
    affected.extend(
        persisted
            .listeners
            .iter()
            .chain(&candidate.listeners)
            .filter(|listener| listener_uses_reference(listener, &references))
            .map(|listener| listener.id),
    );
}

fn listener_uses_reference(
    listener: &ProxyListener,
    references: &BTreeSet<CertificateReferenceId>,
) -> bool {
    let encoded =
        serde_json::to_value(listener).expect("validated ProxyListener serialization cannot fail");
    contains_reference(&encoded, references)
}

fn contains_reference(
    value: &serde_json::Value,
    references: &BTreeSet<CertificateReferenceId>,
) -> bool {
    match value {
        serde_json::Value::String(value) => references
            .iter()
            .any(|reference| reference.to_string() == *value),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| contains_reference(value, references)),
        serde_json::Value::Object(values) => values
            .values()
            .any(|value| contains_reference(value, references)),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            false
        }
    }
}

fn lift_android_route_changes(
    persisted: &ProxyWorkspace,
    candidate: &ProxyWorkspace,
    affected: &mut BTreeSet<ListenerId>,
) {
    let persisted_profiles = persisted
        .android_network_profiles
        .iter()
        .map(|profile| (profile.id.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let candidate_profiles = candidate
        .android_network_profiles
        .iter()
        .map(|profile| (profile.id.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    for profile_id in persisted_profiles.keys().chain(candidate_profiles.keys()) {
        let persisted = persisted_profiles.get(profile_id).copied();
        let candidate = candidate_profiles.get(profile_id).copied();
        if persisted == candidate {
            continue;
        }
        affected.extend(
            persisted
                .into_iter()
                .chain(candidate)
                .flat_map(|profile| &profile.proxy_routes)
                .map(|route| route.listener_id),
        );
    }
}
