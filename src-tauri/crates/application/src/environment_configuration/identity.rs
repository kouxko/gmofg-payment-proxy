use std::sync::Arc;

use crate::{ListenerId, ProtocolDocumentRuleId, WorkspaceId};

pub(crate) trait EnvironmentIdentityAllocatorPort: Send + Sync {
    fn allocate_workspace_id(&self) -> WorkspaceId;

    fn allocate_listener_id(&self, candidate_index: usize, alias: &str) -> ListenerId;

    fn allocate_http_rule(&self, candidate_index: usize) -> (uuid::Uuid, u64);

    fn allocate_protocol_rule(&self, candidate_index: usize) -> (ProtocolDocumentRuleId, u64);

    fn allocate_android_profile_id(&self, candidate_index: usize) -> String;
}

#[derive(Clone)]
pub struct EnvironmentIdentityAllocator {
    inner: Arc<dyn EnvironmentIdentityAllocatorPort>,
}

impl EnvironmentIdentityAllocator {
    #[must_use]
    pub fn random() -> Self {
        Self {
            inner: Arc::new(RandomEnvironmentIdentityAllocator),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_port(inner: Arc<dyn EnvironmentIdentityAllocatorPort>) -> Self {
        Self { inner }
    }

    pub(crate) fn port(&self) -> &dyn EnvironmentIdentityAllocatorPort {
        self.inner.as_ref()
    }
}

impl std::fmt::Debug for EnvironmentIdentityAllocator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentIdentityAllocator")
            .finish_non_exhaustive()
    }
}

struct RandomEnvironmentIdentityAllocator;

impl EnvironmentIdentityAllocatorPort for RandomEnvironmentIdentityAllocator {
    fn allocate_workspace_id(&self) -> WorkspaceId {
        WorkspaceId::new()
    }

    fn allocate_listener_id(&self, _candidate_index: usize, _alias: &str) -> ListenerId {
        ListenerId::new()
    }

    fn allocate_http_rule(&self, candidate_index: usize) -> (uuid::Uuid, u64) {
        (uuid::Uuid::new_v4(), created_order(candidate_index))
    }

    fn allocate_protocol_rule(&self, candidate_index: usize) -> (ProtocolDocumentRuleId, u64) {
        (
            ProtocolDocumentRuleId::new(),
            created_order(candidate_index),
        )
    }

    fn allocate_android_profile_id(&self, _candidate_index: usize) -> String {
        format!("android-profile-{}", uuid::Uuid::new_v4())
    }
}

fn created_order(candidate_index: usize) -> u64 {
    u64::try_from(candidate_index)
        .unwrap_or(u64::MAX - 1)
        .saturating_add(1)
}
