use std::sync::Arc;

use intercept_proxy_domain::{ListenerId, ProxyWorkspace, Revision, RuleDefinition, WorkspaceId};

use super::{
    external_relay::ExternalSocketRuntimeSnapshot,
    http_protocol_pipeline::HttpProtocolRuntimeSnapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimeRuleBundleBaseline {
    Stopped,
    Running(uuid::Uuid),
}

#[cfg(test)]
mod baseline_tests {
    use super::RuntimeRuleBundleBaseline::{Running, Stopped};

    #[test]
    fn every_runtime_state_transition_changes_the_rule_bundle_baseline() {
        let first_run = uuid::Uuid::new_v4();
        let second_run = uuid::Uuid::new_v4();

        assert_ne!(Stopped, Running(first_run));
        assert_ne!(Running(first_run), Stopped);
        assert_ne!(Running(first_run), Running(second_run));
        assert_eq!(Stopped, Stopped);
        assert_eq!(Running(first_run), Running(first_run));
    }
}

#[derive(Debug)]
pub(super) struct RuntimeRuleBundle {
    listener_id: ListenerId,
    pub(super) workspace_id: WorkspaceId,
    workspace_revision: Revision,
    rule_definitions: Vec<RuleDefinition>,
    pub(super) external_socket_programs: Option<Arc<ExternalSocketRuntimeSnapshot>>,
    pub(super) http_programs: Option<Arc<HttpProtocolRuntimeSnapshot>>,
    pub(super) transaction: Arc<tokio::sync::Mutex<()>>,
}

impl RuntimeRuleBundle {
    pub(super) fn new(
        listener_id: ListenerId,
        workspace: ProxyWorkspace,
        external_socket_programs: Option<Arc<ExternalSocketRuntimeSnapshot>>,
        http_programs: Option<Arc<HttpProtocolRuntimeSnapshot>>,
        transaction: Arc<tokio::sync::Mutex<()>>,
    ) -> Self {
        Self {
            listener_id,
            workspace_id: workspace.id,
            workspace_revision: workspace.revision,
            rule_definitions: workspace.rule_definitions,
            external_socket_programs,
            http_programs,
            transaction,
        }
    }

    pub(super) fn publish_workspace(&mut self, workspace: &ProxyWorkspace) {
        debug_assert_eq!(self.workspace_id, workspace.id);
        debug_assert!(
            workspace
                .listeners
                .iter()
                .any(|listener| listener.id == self.listener_id)
        );
        self.workspace_revision = workspace.revision;
        self.rule_definitions
            .clone_from(&workspace.rule_definitions);
    }
}
