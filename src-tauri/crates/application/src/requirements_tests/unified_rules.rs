use super::*;
use intercept_proxy_domain::{
    ConditionTree, HttpAction as DomainRuleAction, HttpRuleContent, MatchField, MatchOperator,
    TerminalAction, UnifiedAction,
};

fn http_tree(conditions: Vec<Condition>) -> ConditionTree {
    ConditionTree::All(conditions.into_iter().map(ConditionTree::Leaf).collect())
}

fn http_actions(actions: Vec<DomainRuleAction>) -> Vec<UnifiedAction> {
    actions.into_iter().map(UnifiedAction::from).collect()
}

#[derive(Debug, Default)]
struct FailFirstUnifiedReplacementRuntime {
    inner: InMemoryListenerRuntime,
    replacements: parking_lot::Mutex<Vec<ProxyWorkspace>>,
}

#[async_trait]
impl ListenerRuntimePort for FailFirstUnifiedReplacementRuntime {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        self.inner.statuses().await
    }

    async fn start(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        self.inner.start(workspace, listener).await
    }

    async fn stop(&self, listener_id: ListenerId) -> AppResult<ListenerStatusViewModel> {
        self.inner.stop(listener_id).await
    }

    async fn replace_rule_definitions(
        &self,
        workspace: ProxyWorkspace,
        listener_id: ListenerId,
    ) -> AppResult<()> {
        let replacement_index = {
            let mut replacements = self.replacements.lock();
            replacements.push(workspace.clone());
            replacements.len()
        };
        if replacement_index == 1 {
            return Err(AppError::new(
                "RULE_RUNTIME_REPLACE_FAILED",
                "injected replacement failure",
            ));
        }
        self.inner
            .replace_rule_definitions(workspace, listener_id)
            .await
    }

    async fn test_upstream_connection(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        self.inner
            .test_upstream_connection(workspace, listener)
            .await
    }

    async fn test_upstream_tls(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        self.inner.test_upstream_tls(workspace, listener).await
    }
}

#[tokio::test]
async fn unified_copy_persists_an_independent_rule_with_monotonic_order() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let selected = application
        .workspace_list()
        .await
        .unwrap()
        .into_iter()
        .find(|workspace| workspace.selected)
        .unwrap();
    let workspace = application.workspace_get(selected.id).await.unwrap();
    let source = application
        .rule_definition_save(RuleDefinitionSaveInput {
            rule_id: None,
            expected_revision: None,
            draft: RuleDefinitionDraft {
                name: "源规则".into(),
                enabled: true,
                priority: 10,
                listener_id: workspace.listeners[0].id,
                stage: RuleStage::ProxyToUpstream,
                one_shot: false,
                content: RuleContent::Http(HttpRuleContent {
                    description: "source".into(),
                    condition: http_tree(vec![Condition::NthHit { count: 1 }]),
                    actions: http_actions(vec![DomainRuleAction::Delay { milliseconds: 10 }]),
                    document: None,
                }),
            },
        })
        .await
        .unwrap();

    let copied = application
        .rule_definition_copy(source.rule_id())
        .await
        .unwrap();

    assert_ne!(copied.rule_id(), source.rule_id());
    assert_eq!(copied.name(), "源规则（副本）");
    assert!(copied.created_order() > source.created_order());
    assert_eq!(copied.content(), source.content());
    assert_eq!(application.rule_definition_list().await.unwrap().len(), 2);
}

#[test]
fn unified_http_factories_return_domain_condition_and_action_types() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let condition = application
        .rule_definition_condition_draft(RuleConditionKind::Field, MessageStage::Request)
        .unwrap();
    assert!(matches!(
        condition,
        Condition::Http {
            field: MatchField::PathOrRequestType,
            ..
        }
    ));
    assert_eq!(
        application
            .rule_definition_action_draft(RuleActionKind::MockResponse, MessageStage::Request)
            .unwrap(),
        DomainRuleAction::Terminal(intercept_proxy_domain::TerminalAction::MockResponse {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body_bytes: b"{}".to_vec(),
        })
    );
}

#[tokio::test]
async fn unified_runtime_failure_restores_business_state_with_rebased_revision() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let runtime = Arc::new(FailFirstUnifiedReplacementRuntime::default());
    let application = application_with_workspace_ports_and_listener_runtime(
        ports,
        Arc::clone(&workspaces),
        runtime.clone(),
    );
    let selected = application
        .workspace_list()
        .await
        .unwrap()
        .into_iter()
        .find(|workspace| workspace.selected)
        .unwrap();
    let before = application.workspace_get(selected.id).await.unwrap();

    let error = application
        .rule_definition_save(RuleDefinitionSaveInput {
            rule_id: None,
            expected_revision: None,
            draft: RuleDefinitionDraft {
                name: "应回滚".into(),
                enabled: true,
                priority: 10,
                listener_id: before.listeners[0].id,
                stage: RuleStage::ProxyToUpstream,
                one_shot: false,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition: http_tree(vec![Condition::NthHit { count: 1 }]),
                    actions: http_actions(vec![DomainRuleAction::Delay { milliseconds: 10 }]),
                    document: None,
                }),
            },
        })
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "RULE_RUNTIME_REPLACE_FAILED");
    let after = application.workspace_get(before.id).await.unwrap();
    assert_eq!(after.rule_definitions, before.rule_definitions);
    assert_eq!(
        after.rule_created_order_high_water,
        before.rule_created_order_high_water
    );
    assert!(after.revision > before.revision);
    let replacements = runtime.replacements.lock();
    assert_eq!(replacements.len(), 2);
    assert_eq!(replacements[1].rule_definitions, before.rule_definitions);
    assert_eq!(replacements[1].revision, after.revision);
}

#[tokio::test]
async fn unified_save_rejects_every_invalid_http_runtime_shape_without_persistence() {
    let invalid_shapes = [
        (
            RuleStage::ProxyToUpstream,
            vec![Condition::NthHit { count: 0 }],
            vec![DomainRuleAction::Delay { milliseconds: 10 }],
        ),
        (
            RuleStage::ProxyToUpstream,
            vec![Condition::Http {
                field: MatchField::PathOrRequestType,
                operator: MatchOperator::Regex("[".into()),
            }],
            vec![DomainRuleAction::Delay { milliseconds: 10 }],
        ),
        (
            RuleStage::ProxyToApp,
            Vec::new(),
            vec![DomainRuleAction::Terminal(TerminalAction::MockResponse {
                status: 200,
                headers: Vec::new(),
                body_bytes: Vec::new(),
            })],
        ),
        (RuleStage::ProxyToUpstream, Vec::new(), Vec::new()),
    ];

    for (stage, conditions, actions) in invalid_shapes {
        let workspaces = Arc::new(InMemoryWorkspaceStore::default());
        let application = application_with_workspace_ports(
            Arc::new(FakePorts::default()),
            Arc::clone(&workspaces),
        );
        let selected = application.workspace_list().await.unwrap()[0].clone();
        let before = application.workspace_get(selected.id).await.unwrap();
        let error = application
            .rule_definition_save(RuleDefinitionSaveInput {
                rule_id: None,
                expected_revision: None,
                draft: RuleDefinitionDraft {
                    name: "invalid HTTP".into(),
                    enabled: false,
                    priority: 10,
                    listener_id: before.listeners[0].id,
                    stage,
                    one_shot: false,
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        condition: http_tree(conditions),
                        actions: http_actions(actions),
                        document: None,
                    }),
                },
            })
            .await
            .unwrap_err();
        assert_eq!(error.view_model.code, "RULE_INVALID");
        assert_eq!(application.workspace_get(before.id).await.unwrap(), before);
    }
}
