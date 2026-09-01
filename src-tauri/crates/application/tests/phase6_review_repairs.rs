use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppErrorDiagnosticViewModel, AppResult, RuleChainCommitPort, RuleChainCommitRequest,
    RuleChainHttpPort, RuleChainInput, RuleChainPlan, RuleChainPlanEntry, RuleChainTransaction,
    WorkingHttpMessage,
};
use intercept_proxy_domain::{
    Condition, Document, DocumentValue, HttpAction, NthCounterSnapshot, Revision, RuleId,
    RuleLifecycle, RuleLifecycleSnapshot, RuleProgramEntry, TerminalIdentity, UnifiedAction,
};

#[derive(Debug, Default)]
struct HttpPort {
    error: Option<AppError>,
}

impl RuleChainHttpPort for HttpPort {
    fn matches(
        &self,
        _: &WorkingHttpMessage,
        _: &intercept_proxy_domain::MatchField,
        _: &intercept_proxy_domain::MatchOperator,
    ) -> AppResult<bool> {
        self.error.clone().map_or(Ok(false), Err)
    }

    fn apply(&self, _: &mut WorkingHttpMessage, _: &HttpAction) -> AppResult<()> {
        Ok(())
    }

    fn encode_document(&self, _: &Document, _: &mut WorkingHttpMessage) -> AppResult<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct CommitPort {
    attempts: AtomicUsize,
    requests: Mutex<Vec<RuleChainCommitRequest>>,
}

#[async_trait]
impl RuleChainCommitPort for CommitPort {
    async fn commit(&self, request: RuleChainCommitRequest) -> AppResult<u64> {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        self.requests.lock().unwrap().push(request.clone());
        Ok(request.expected_collection_revision + 1)
    }
}

fn terminal(ip: &str, certificate: &str) -> TerminalIdentity {
    TerminalIdentity {
        source_ip: ip.into(),
        certificate_sha256: certificate.into(),
    }
}

fn program(rule_id: RuleId, conditions: Vec<Condition>) -> RuleProgramEntry {
    RuleProgramEntry::new(rule_id, 1, 1, conditions, vec![UnifiedAction::RecordMatch]).unwrap()
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

fn plan_entry(
    rule_id: RuleId,
    terminal: &TerminalIdentity,
    attempts: u64,
    nth: u64,
) -> RuleChainPlanEntry {
    RuleChainPlanEntry::new(
        program(rule_id, vec![Condition::NthHit { count: nth }]),
        lifecycle(rule_id),
        NthCounterSnapshot {
            rule_id,
            terminal: terminal.clone(),
            attempts,
        },
    )
    .unwrap()
}

fn input(terminal: TerminalIdentity, plan: RuleChainPlan) -> RuleChainInput {
    RuleChainInput {
        expected_collection_revision: 1,
        terminal,
        message: WorkingHttpMessage::new("/"),
        document: Document::new(DocumentValue::Null(())),
        plan,
        evaluated_at: Utc::now(),
    }
}

#[tokio::test]
async fn nth_two_advances_on_successful_miss_then_matches_same_terminal() {
    let rule_id = RuleId::new();
    let identity = terminal("10.0.0.1", "cert-a");
    let commit = Arc::new(CommitPort::default());
    let transaction = RuleChainTransaction::new(Arc::new(HttpPort::default()), commit.clone());

    let first = transaction
        .execute(input(
            identity.clone(),
            RuleChainPlan::new(vec![plan_entry(rule_id, &identity, 0, 2)]).unwrap(),
        ))
        .await
        .unwrap();
    assert!(first.matched_rule_ids.is_empty());
    let first_request = commit.requests.lock().unwrap()[0].clone();
    assert_eq!(first_request.deltas[0].hit_count_increment, 0);
    assert_eq!(
        first_request.deltas[0]
            .nth_counter_advance
            .as_ref()
            .unwrap()
            .expected_attempts,
        0
    );

    let second = transaction
        .execute(input(
            identity.clone(),
            RuleChainPlan::new(vec![plan_entry(rule_id, &identity, 1, 2)]).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(second.matched_rule_ids, vec![rule_id]);
    assert_eq!(commit.attempts.load(Ordering::Acquire), 2);
}

#[test]
fn nth_snapshot_is_isolated_by_ip_and_certificate() {
    let rule_id = RuleId::new();
    let a = terminal("10.0.0.1", "cert-a");
    let other_ip = terminal("10.0.0.2", "cert-a");
    let other_cert = terminal("10.0.0.1", "cert-b");
    assert_ne!(
        NthCounterSnapshot {
            rule_id,
            terminal: a,
            attempts: 1
        },
        NthCounterSnapshot {
            rule_id,
            terminal: other_ip,
            attempts: 1
        }
    );
    assert_ne!(
        NthCounterSnapshot {
            rule_id,
            terminal: terminal("10.0.0.1", "cert-a"),
            attempts: 1
        },
        NthCounterSnapshot {
            rule_id,
            terminal: other_cert,
            attempts: 1
        }
    );
}

#[test]
fn plan_rejects_mismatched_and_duplicate_rule_owners_before_execution() {
    let identity = terminal("10.0.0.1", "cert");
    let program_id = RuleId::new();
    let lifecycle_id = RuleId::new();
    let error = RuleChainPlanEntry::new(
        program(program_id, vec![Condition::NthHit { count: 1 }]),
        lifecycle(lifecycle_id),
        NthCounterSnapshot {
            rule_id: program_id,
            terminal: identity.clone(),
            attempts: 0,
        },
    )
    .unwrap_err();
    assert_eq!(error.view_model.code, "RULE_INVALID");

    let first = plan_entry(program_id, &identity, 0, 1);
    let second = plan_entry(program_id, &identity, 0, 1);
    assert_eq!(
        RuleChainPlan::new(vec![first, second])
            .unwrap_err()
            .view_model
            .code,
        "RULE_INVALID"
    );
}

#[tokio::test]
async fn condition_error_preserves_the_complete_application_error() {
    let mut fields = BTreeMap::new();
    fields.insert("condition.field".into(), vec!["invalid".into()]);
    let expected = AppError::field("PACKAGE_UNAVAILABLE", "package failed", fields)
        .retryable("reconnect")
        .runtime_context("rule-entity", Some(uuid::Uuid::nil()))
        .diagnostic(AppErrorDiagnosticViewModel {
            file: Some("manifest.json".into()),
            field: Some("condition".into()),
            line: Some(3),
            column: Some(7),
            entry: Some("match".into()),
        });
    let rule_id = RuleId::new();
    let identity = terminal("10.0.0.1", "cert");
    let entry = RuleChainPlanEntry::new(
        program(
            rule_id,
            vec![Condition::Http {
                field: intercept_proxy_domain::MatchField::RequestTarget,
                operator: intercept_proxy_domain::MatchOperator::Equals("/".into()),
            }],
        ),
        lifecycle(rule_id),
        NthCounterSnapshot {
            rule_id,
            terminal: identity.clone(),
            attempts: 0,
        },
    )
    .unwrap();
    let commit = Arc::new(CommitPort::default());
    let transaction = RuleChainTransaction::new(
        Arc::new(HttpPort {
            error: Some(expected.clone()),
        }),
        commit.clone(),
    );

    let actual = transaction
        .execute(input(identity, RuleChainPlan::new(vec![entry]).unwrap()))
        .await
        .unwrap_err();
    assert_eq!(actual, expected);
    assert_eq!(commit.attempts.load(Ordering::Acquire), 0);
}

#[derive(Debug)]
struct NoPortCalls;

impl RuleChainHttpPort for NoPortCalls {
    fn matches(
        &self,
        _: &WorkingHttpMessage,
        _: &intercept_proxy_domain::MatchField,
        _: &intercept_proxy_domain::MatchOperator,
    ) -> AppResult<bool> {
        panic!("terminal mismatch must fail before HTTP match")
    }

    fn apply(&self, _: &mut WorkingHttpMessage, _: &HttpAction) -> AppResult<()> {
        panic!("terminal mismatch must fail before HTTP action")
    }

    fn encode_document(&self, _: &Document, _: &mut WorkingHttpMessage) -> AppResult<()> {
        panic!("terminal mismatch must fail before encode")
    }
}

#[tokio::test]
async fn terminal_mismatch_fails_before_any_port_or_commit_call() {
    let rule_id = RuleId::new();
    let planned_terminal = terminal("10.0.0.1", "cert");
    let entry = plan_entry(rule_id, &planned_terminal, 0, 1);
    let commit = Arc::new(CommitPort::default());
    let transaction = RuleChainTransaction::new(Arc::new(NoPortCalls), commit.clone());

    let error = transaction
        .execute(input(
            terminal("10.0.0.2", "cert"),
            RuleChainPlan::new(vec![entry]).unwrap(),
        ))
        .await
        .expect_err("terminal mismatch");

    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert_eq!(commit.attempts.load(Ordering::Acquire), 0);
}
