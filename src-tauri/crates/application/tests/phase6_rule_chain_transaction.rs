use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use intercept_proxy_application::{
    AppError, AppResult, RuleChainCommitPort, RuleChainCommitRequest, RuleChainHttpPort,
    RuleChainInput, RuleChainPlan, RuleChainPlanEntry, RuleChainTransaction, WorkingHttpMessage,
};
use intercept_proxy_domain::{
    Condition, ConditionTree, Document, DocumentMutation, DocumentValue, HttpAction, JsonPointer,
    NthCounterSnapshot, Revision, RuleId, RuleLifecycle, RuleLifecycleSnapshot, RuleProgramEntry,
    TerminalIdentity, UnifiedAction,
};

#[derive(Debug, Default)]
struct HttpPort {
    fail_match: bool,
    fail_apply: bool,
    fail_encode: bool,
}

impl RuleChainHttpPort for HttpPort {
    fn matches(
        &self,
        message: &WorkingHttpMessage,
        _: &intercept_proxy_domain::MatchField,
        _: &intercept_proxy_domain::MatchOperator,
    ) -> AppResult<bool> {
        if self.fail_match {
            return Err(AppError::new("RULE_INVALID", "condition failed"));
        }
        Ok(message
            .headers
            .get("x-phase")
            .is_some_and(|value| value == "6"))
    }

    fn apply(&self, message: &mut WorkingHttpMessage, action: &HttpAction) -> AppResult<()> {
        if self.fail_apply {
            return Err(AppError::new("RULE_INVALID", "action failed"));
        }
        if let HttpAction::SetHeader { name, value } = action {
            message.headers.insert(name.clone(), value.clone());
        }
        Ok(())
    }

    fn encode_document(&self, _: &Document, _: &mut WorkingHttpMessage) -> AppResult<()> {
        if self.fail_encode {
            return Err(AppError::new("BODY_ENCODE_FAILED", "encode failed"));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct CommitPort {
    attempts: AtomicUsize,
    fail: bool,
    expected_deltas: usize,
}

#[async_trait]
impl RuleChainCommitPort for CommitPort {
    async fn commit(&self, request: RuleChainCommitRequest) -> AppResult<u64> {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        assert_eq!(request.deltas.len(), self.expected_deltas);
        if self.fail {
            Err(AppError::new("REVISION_CONFLICT", "conflict"))
        } else {
            Ok(request.expected_collection_revision + 1)
        }
    }
}

fn rule(priority: i32, condition: ConditionTree, actions: Vec<UnifiedAction>) -> RuleProgramEntry {
    RuleProgramEntry::new(RuleId::new(), priority, 1, condition, actions).expect("rule")
}

#[tokio::test]
async fn transaction_exposes_prior_mutations_only_after_single_commit() {
    let first = rule(
        1,
        ConditionTree::Leaf(Condition::Document {
            path: JsonPointer::parse("/state").unwrap(),
            predicate: intercept_proxy_domain::DocumentPredicate::String(
                intercept_proxy_domain::StringPredicate {
                    operator: intercept_proxy_domain::StringOperator::Equal,
                    value: "before".into(),
                },
            ),
        }),
        vec![
            UnifiedAction::Document(DocumentMutation::Set {
                path: JsonPointer::parse("/state").unwrap(),
                value: DocumentValue::String("after".into()),
            }),
            UnifiedAction::Http(HttpAction::SetHeader {
                name: "x-phase".into(),
                value: "6".into(),
            }),
        ],
    );
    let second = rule(
        2,
        ConditionTree::all(vec![
            ConditionTree::Leaf(Condition::Document {
                path: JsonPointer::parse("/state").unwrap(),
                predicate: intercept_proxy_domain::DocumentPredicate::String(
                    intercept_proxy_domain::StringPredicate {
                        operator: intercept_proxy_domain::StringOperator::Equal,
                        value: "after".into(),
                    },
                ),
            }),
            ConditionTree::Leaf(Condition::Http {
                field: intercept_proxy_domain::MatchField::PathOrRequestType,
                operator: intercept_proxy_domain::MatchOperator::Equals("ignored".into()),
            }),
        ])
        .unwrap(),
        vec![UnifiedAction::RecordMatch],
    );
    let commit = Arc::new(CommitPort {
        attempts: AtomicUsize::new(0),
        fail: false,
        expected_deltas: 2,
    });
    let transaction = RuleChainTransaction::new(Arc::new(HttpPort::default()), commit.clone());
    let input = RuleChainInput {
        expected_collection_revision: 7,
        message: WorkingHttpMessage::new("/before"),
        document: Document::parse_json(r#"{"state":"before"}"#).unwrap(),
        terminal: terminal_identity(),
        plan: RuleChainPlan::new(vec![plan_entry(first), plan_entry(second)]).unwrap(),
        evaluated_at: Utc.with_ymd_and_hms(2026, 8, 30, 12, 2, 0).unwrap(),
    };

    let output = transaction.execute(input).await.expect("committed");
    assert_eq!(commit.attempts.load(Ordering::Acquire), 1);
    assert_eq!(output.collection_revision, 8);
    assert_eq!(output.message.headers.get("x-phase"), Some(&"6".into()));
    assert_eq!(output.matched_rule_ids.len(), 2);
    assert_eq!(output.trace.len(), 2);
    assert!(output.trace.iter().all(|entry| entry.matched));
}

#[tokio::test]
async fn commit_conflict_returns_no_partial_output_and_is_not_retried() {
    let commit = Arc::new(CommitPort {
        attempts: AtomicUsize::new(0),
        fail: true,
        expected_deltas: 1,
    });
    let transaction = RuleChainTransaction::new(Arc::new(HttpPort::default()), commit.clone());
    let input = RuleChainInput {
        expected_collection_revision: 9,
        message: WorkingHttpMessage::new("/original"),
        document: Document::new(DocumentValue::Null(())),
        terminal: terminal_identity(),
        plan: RuleChainPlan::new(vec![plan_entry(rule(
            1,
            ConditionTree::Leaf(Condition::NthHit { count: 1 }),
            vec![UnifiedAction::RecordMatch],
        ))])
        .unwrap(),
        evaluated_at: Utc::now(),
    };

    let error = transaction.execute(input).await.expect_err("conflict");
    assert_eq!(error.view_model.code, "REVISION_CONFLICT");
    assert_eq!(commit.attempts.load(Ordering::Acquire), 1);
}

#[derive(Debug)]
struct CommitValidationPort {
    attempts: AtomicUsize,
}

#[async_trait]
impl RuleChainCommitPort for CommitValidationPort {
    async fn commit(&self, _: RuleChainCommitRequest) -> AppResult<u64> {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        Err(AppError::new("RULE_INVALID", "delta validation failed"))
    }
}

#[tokio::test]
async fn commit_validation_failure_returns_no_partial_output_and_is_not_retried() {
    let commit = Arc::new(CommitValidationPort {
        attempts: AtomicUsize::new(0),
    });
    let transaction = RuleChainTransaction::new(Arc::new(HttpPort::default()), commit.clone());

    let error = transaction
        .execute(single_input(rule(
            1,
            ConditionTree::Leaf(Condition::NthHit { count: 1 }),
            vec![UnifiedAction::RecordMatch],
        )))
        .await
        .expect_err("invalid lifecycle delta");

    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert_eq!(commit.attempts.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn condition_action_encode_and_cancel_fail_before_commit() {
    let http_condition = || {
        ConditionTree::Leaf(Condition::Http {
            field: intercept_proxy_domain::MatchField::PathOrRequestType,
            operator: intercept_proxy_domain::MatchOperator::Equals("x".into()),
        })
    };
    let http_action = || {
        vec![UnifiedAction::Http(HttpAction::SetHeader {
            name: "x".into(),
            value: "y".into(),
        })]
    };
    for (http, expected_code, condition, actions) in [
        (
            HttpPort {
                fail_match: true,
                ..HttpPort::default()
            },
            "RULE_INVALID",
            http_condition(),
            http_action(),
        ),
        (
            HttpPort {
                fail_apply: true,
                ..HttpPort::default()
            },
            "RULE_INVALID",
            ConditionTree::Leaf(Condition::NthHit { count: 1 }),
            http_action(),
        ),
        (
            HttpPort {
                fail_encode: true,
                ..HttpPort::default()
            },
            "BODY_ENCODE_FAILED",
            ConditionTree::Leaf(Condition::NthHit { count: 1 }),
            vec![UnifiedAction::RecordMatch],
        ),
    ] {
        let commit = Arc::new(CommitPort {
            attempts: AtomicUsize::new(0),
            fail: false,
            expected_deltas: 1,
        });
        let transaction = RuleChainTransaction::new(Arc::new(http), commit.clone());
        let error = transaction
            .execute(single_input(rule(1, condition, actions)))
            .await
            .expect_err("failure");
        assert_eq!(error.view_model.code, expected_code);
        assert_eq!(commit.attempts.load(Ordering::Acquire), 0);
    }

    let commit = Arc::new(CommitPort {
        attempts: AtomicUsize::new(0),
        fail: false,
        expected_deltas: 1,
    });
    let transaction = RuleChainTransaction::new(Arc::new(HttpPort::default()), commit.clone());
    let cancellation = tokio_util::sync::CancellationToken::new();
    cancellation.cancel();
    let error = transaction
        .execute_cancellable(
            single_input(rule(
                1,
                ConditionTree::Leaf(Condition::NthHit { count: 1 }),
                vec![UnifiedAction::RecordMatch],
            )),
            &cancellation,
        )
        .await
        .expect_err("cancelled");
    assert_eq!(error.view_model.code, "RULE_EXECUTION_CANCELLED");
    assert_eq!(commit.attempts.load(Ordering::Acquire), 0);
}

#[derive(Debug)]
struct CasCommitPort {
    revision: Mutex<u64>,
    attempts: AtomicUsize,
}

#[async_trait]
impl RuleChainCommitPort for CasCommitPort {
    async fn commit(&self, request: RuleChainCommitRequest) -> AppResult<u64> {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        let mut revision = self.revision.lock().unwrap();
        if *revision != request.expected_collection_revision {
            return Err(AppError::new("REVISION_CONFLICT", "conflict"));
        }
        *revision += 1;
        Ok(*revision)
    }
}

#[tokio::test]
async fn concurrent_same_revision_has_one_winner_and_one_single_conflict() {
    let commit = Arc::new(CasCommitPort {
        revision: Mutex::new(4),
        attempts: AtomicUsize::new(0),
    });
    let transaction = Arc::new(RuleChainTransaction::new(
        Arc::new(HttpPort::default()),
        commit.clone(),
    ));
    let make = || {
        let mut input = single_input(rule(
            1,
            ConditionTree::Leaf(Condition::NthHit { count: 1 }),
            vec![UnifiedAction::RecordMatch],
        ));
        input.expected_collection_revision = 4;
        input
    };
    let (left, right) = tokio::join!(transaction.execute(make()), transaction.execute(make()));
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let error = left.err().or_else(|| right.err()).expect("one conflict");
    assert_eq!(error.view_model.code, "REVISION_CONFLICT");
    assert_eq!(commit.attempts.load(Ordering::Acquire), 2);
}

#[tokio::test]
async fn terminal_is_pending_until_commit_and_stops_lower_rules() {
    let commit = Arc::new(CommitPort {
        attempts: AtomicUsize::new(0),
        fail: false,
        expected_deltas: 1,
    });
    let transaction = RuleChainTransaction::new(Arc::new(HttpPort::default()), commit.clone());
    let terminal = intercept_proxy_domain::TerminalAction::DisconnectBeforeUpstream;
    let mut input = single_input(rule(
        1,
        ConditionTree::Leaf(Condition::NthHit { count: 1 }),
        vec![UnifiedAction::Terminal(terminal.clone())],
    ));
    let second = plan_entry(rule(
        2,
        ConditionTree::Leaf(Condition::NthHit { count: 1 }),
        vec![UnifiedAction::RecordMatch],
    ));
    let first = plan_entry(rule(
        1,
        ConditionTree::Leaf(Condition::NthHit { count: 1 }),
        vec![UnifiedAction::Terminal(terminal.clone())],
    ));
    input.plan = RuleChainPlan::new(vec![first, second]).unwrap();

    let output = transaction
        .execute(input)
        .await
        .expect("commit before terminal output");
    assert_eq!(commit.attempts.load(Ordering::Acquire), 1);
    assert_eq!(output.terminal_action, Some(terminal));
    assert_eq!(output.matched_rule_ids.len(), 1);
}

fn single_input(entry: RuleProgramEntry) -> RuleChainInput {
    RuleChainInput {
        expected_collection_revision: 1,
        message: WorkingHttpMessage::new("/original"),
        document: Document::new(DocumentValue::Null(())),
        terminal: terminal_identity(),
        plan: RuleChainPlan::new(vec![plan_entry(entry)]).unwrap(),
        evaluated_at: Utc::now(),
    }
}

fn lifecycle(rule_id: RuleId) -> RuleLifecycleSnapshot {
    RuleLifecycleSnapshot {
        rule_id,
        revision: Revision::INITIAL,
        enabled: true,
        one_shot: false,
        lifecycle: RuleLifecycle::default(),
    }
}

fn plan_entry(entry: RuleProgramEntry) -> RuleChainPlanEntry {
    let snapshot = lifecycle(entry.rule_id());
    let nth = NthCounterSnapshot {
        rule_id: entry.rule_id(),
        terminal: terminal_identity(),
        attempts: 0,
    };
    RuleChainPlanEntry::new(entry, snapshot, nth).unwrap()
}

fn terminal_identity() -> TerminalIdentity {
    TerminalIdentity {
        source_ip: "127.0.0.1".into(),
        certificate_sha256: "test-cert".into(),
    }
}
