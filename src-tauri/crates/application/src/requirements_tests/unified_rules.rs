use super::*;
use intercept_proxy_domain::{
    HttpAction as DomainRuleAction, HttpRuleContent, MatchField, MatchOperator, TerminalAction,
    UnifiedAction,
};

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
        workspaces: &dyn WorkspaceRepositoryPort,
        workspace: ProxyWorkspace,
        listener_id: ListenerId,
    ) -> AppResult<ProxyWorkspace> {
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
            .replace_rule_definitions(workspaces, workspace, listener_id)
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
                content: RuleContent::Http(HttpRuleContent {
                    description: "source".into(),
                    condition: Condition::Http {
                        field: MatchField::Method,
                        operator: MatchOperator::Equals("GET".into()),
                    },
                    action: UnifiedAction::from(DomainRuleAction::Delay { milliseconds: 10 }),
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
        .rule_definition_http_condition_draft(
            crate::RuleMatchFieldKind::RequestTarget,
            None,
            crate::RuleMatchOperatorKind::Equals,
            "/",
            RuleStage::ProxyToUpstream,
        )
        .unwrap();
    assert!(matches!(
        condition,
        Condition::Http {
            field: MatchField::RequestTarget,
            operator: MatchOperator::Equals(ref value),
        } if value == "/"
    ));
    let header = application
        .rule_definition_http_condition_draft(
            crate::RuleMatchFieldKind::Header,
            Some("/content-type"),
            crate::RuleMatchOperatorKind::Wildcard,
            "application/*",
            RuleStage::ProxyToApp,
        )
        .unwrap();
    assert!(matches!(
        header,
        Condition::Http {
            field: MatchField::Header(ref path),
            operator: MatchOperator::Wildcard(ref value),
        } if path == "/content-type" && value == "application/*"
    ));
    assert!(
        application
            .rule_definition_http_condition_draft(
                crate::RuleMatchFieldKind::Method,
                None,
                crate::RuleMatchOperatorKind::Contains,
                "PO",
                RuleStage::ProxyToUpstream,
            )
            .is_err()
    );
    assert!(
        application
            .rule_definition_http_condition_draft(
                crate::RuleMatchFieldKind::Header,
                None,
                crate::RuleMatchOperatorKind::Equals,
                "application/json",
                RuleStage::ProxyToUpstream,
            )
            .is_err()
    );
    assert!(matches!(
        application
            .rule_definition_document_condition_draft(
                "/customer/*/name",
                crate::RuleLocalDocumentValueType::String,
                crate::RuleLocalDocumentPredicateKind::Equals,
                "\"Alice\"",
            )
            .unwrap(),
        Condition::DocumentPattern { .. }
    ));
    assert_eq!(
        application
            .rule_definition_action_draft(
                crate::RuleHttpActionDraftInput {
                    kind: RuleActionKind::MockResponse,
                    parameters_json: Some(
                        r#"{"status":201,"headers":[["x-test","explicit"]],"body":"body"}"#.into(),
                    ),
                },
                RuleStage::ProxyToUpstream,
            )
            .unwrap(),
        DomainRuleAction::Terminal(intercept_proxy_domain::TerminalAction::MockResponse {
            status: 201,
            headers: vec![("x-test".into(), "explicit".into())],
            body: "body".into(),
        })
    );
}

#[tokio::test]
async fn plain_http_editor_exposes_schema_free_body_document_capability() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let selected = application
        .workspace_list()
        .await
        .unwrap()
        .into_iter()
        .find(|workspace| workspace.selected)
        .unwrap();
    let workspace = application.workspace_get(selected.id).await.unwrap();

    let context = application
        .rule_editor_context(workspace.listeners[0].id)
        .await
        .unwrap();
    let RuleEditorContentContext::Http { stages } = context.content else {
        panic!("default listener must be HTTP");
    };

    assert_eq!(stages.len(), 2);
    assert!(stages.iter().all(|stage| stage.document_fields.is_empty()));
    assert!(stages.iter().all(|stage| {
        stage.document_common_actions == vec![RuleCommonActionCapability::RecordMatch]
    }));
}

#[tokio::test]
async fn unified_runtime_failure_does_not_persist_or_advance_revision() {
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
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition: Condition::Http {
                        field: MatchField::Method,
                        operator: MatchOperator::Equals("GET".into()),
                    },
                    action: UnifiedAction::from(DomainRuleAction::Delay { milliseconds: 10 }),
                }),
            },
        })
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "RULE_RUNTIME_REPLACE_FAILED");
    let after = application.workspace_get(before.id).await.unwrap();
    assert_eq!(after, before);
    let replacements = runtime.replacements.lock();
    assert_eq!(replacements.len(), 1);
}

#[tokio::test]
async fn unified_save_rejects_every_invalid_http_runtime_shape_without_persistence() {
    let invalid_shapes = [
        (
            RuleStage::ProxyToUpstream,
            Condition::Http {
                field: MatchField::Method,
                operator: MatchOperator::Contains("OS".into()),
            },
            DomainRuleAction::Delay { milliseconds: 10 },
        ),
        (
            RuleStage::ProxyToApp,
            Condition::Http {
                field: MatchField::Method,
                operator: MatchOperator::Equals("GET".into()),
            },
            DomainRuleAction::Terminal(TerminalAction::MockResponse {
                status: 200,
                headers: Vec::new(),
                body: String::new(),
            }),
        ),
    ];

    for (stage, condition, action) in invalid_shapes {
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
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        condition,
                        action: UnifiedAction::from(action),
                    }),
                },
            })
            .await
            .unwrap_err();
        assert_eq!(error.view_model.code, "RULE_INVALID");
        assert_eq!(application.workspace_get(before.id).await.unwrap(), before);
    }
}
