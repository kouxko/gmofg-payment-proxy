use std::collections::BTreeSet;

use intercept_proxy_domain::{ListenerId, ProtocolPackageRef, ProxyWorkspace};

use super::lifting;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentResourceChangeKind {
    Added,
    Removed,
    Changed,
    Unchanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvironmentAffectedResourceKey {
    Listener(ListenerId),
    AndroidProfile(String),
    ExactPackage(ProtocolPackageRef),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentAffectedResourceDiff {
    pub key: EnvironmentAffectedResourceKey,
    pub change: EnvironmentResourceChangeKind,
}

pub(super) fn classify(
    persisted: Option<&ProxyWorkspace>,
    candidate: &ProxyWorkspace,
) -> Vec<EnvironmentAffectedResourceDiff> {
    let Some(persisted) = persisted else {
        let mut result = candidate
            .listeners
            .iter()
            .map(|listener| listener_diff(listener.id, EnvironmentResourceChangeKind::Added))
            .collect::<Vec<_>>();
        result.extend(candidate_packages(candidate).into_iter().map(|package| {
            resource_diff(
                EnvironmentAffectedResourceKey::ExactPackage(package),
                EnvironmentResourceChangeKind::Added,
            )
        }));
        return result;
    };

    let lifted = lifting::affected_listener_ids(Some(persisted), candidate)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let listener_ids = persisted
        .listeners
        .iter()
        .chain(&candidate.listeners)
        .map(|listener| listener.id)
        .collect::<BTreeSet<_>>();
    let mut result = listener_ids
        .into_iter()
        .map(|listener_id| {
            let before = persisted
                .listeners
                .iter()
                .find(|item| item.id == listener_id);
            let after = candidate
                .listeners
                .iter()
                .find(|item| item.id == listener_id);
            let change = match (before, after) {
                (None, Some(_)) => EnvironmentResourceChangeKind::Added,
                (Some(_), None) => EnvironmentResourceChangeKind::Removed,
                (Some(left), Some(right)) if left == right && !lifted.contains(&listener_id) => {
                    EnvironmentResourceChangeKind::Unchanged
                }
                (Some(_), Some(_)) => EnvironmentResourceChangeKind::Changed,
                (None, None) => unreachable!("listener id came from the workspace union"),
            };
            listener_diff(listener_id, change)
        })
        .collect::<Vec<_>>();

    let before = candidate_packages(persisted);
    let after = candidate_packages(candidate);
    let mut packages = before.iter().chain(&after).cloned().collect::<Vec<_>>();
    packages.sort_by(package_cmp);
    packages.dedup();
    result.extend(packages.into_iter().map(|package| {
        let change = match (before.contains(&package), after.contains(&package)) {
            (false, true) => EnvironmentResourceChangeKind::Added,
            (true, false) => EnvironmentResourceChangeKind::Removed,
            (true, true) => EnvironmentResourceChangeKind::Unchanged,
            (false, false) => unreachable!("package came from the workspace union"),
        };
        resource_diff(
            EnvironmentAffectedResourceKey::ExactPackage(package),
            change,
        )
    }));
    result
}

fn listener_diff(
    listener_id: ListenerId,
    change: EnvironmentResourceChangeKind,
) -> EnvironmentAffectedResourceDiff {
    resource_diff(
        EnvironmentAffectedResourceKey::Listener(listener_id),
        change,
    )
}

fn resource_diff(
    key: EnvironmentAffectedResourceKey,
    change: EnvironmentResourceChangeKind,
) -> EnvironmentAffectedResourceDiff {
    EnvironmentAffectedResourceDiff { key, change }
}

fn candidate_packages(workspace: &ProxyWorkspace) -> Vec<ProtocolPackageRef> {
    let mut packages = workspace
        .listeners
        .iter()
        .filter_map(super::listener_protocol_package)
        .cloned()
        .collect::<Vec<_>>();
    packages.sort_by(package_cmp);
    packages.dedup();
    packages
}

fn package_cmp(left: &ProtocolPackageRef, right: &ProtocolPackageRef) -> std::cmp::Ordering {
    left.id.as_str().cmp(right.id.as_str()).then_with(|| {
        left.version
            .semantic_cmp(&right.version)
            .then_with(|| left.version.as_str().cmp(right.version.as_str()))
    })
}
