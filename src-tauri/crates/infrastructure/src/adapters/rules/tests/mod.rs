use std::{path::PathBuf, sync::Arc};

use super::*;
use crate::adapters::{FileSelection, NativeFileDialog};
use chrono::TimeZone;
use intercept_proxy_domain::{MatchContext, TerminalIdentity};

#[derive(Debug)]
struct NoDialog;

impl NativeFileDialog for NoDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct StaticOpenDialog {
    path: PathBuf,
}

impl NativeFileDialog for StaticOpenDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        Ok(Some(self.path.clone()))
    }

    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct MutatingOpenDialog {
    path: PathBuf,
    store: Arc<SqliteStore>,
    concurrent_rule: Rule,
}

impl NativeFileDialog for MutatingOpenDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        let snapshot = infra(self.store.load_workspaces())?;
        let selected_id = snapshot.selected_id.expect("selected workspace");
        let record = snapshot
            .records
            .into_iter()
            .find(|record| record.id == selected_id)
            .expect("selected record");
        let mut workspace = decode_workspace_record(record.clone()).expect("workspace");
        workspace.rules.push(self.concurrent_rule.clone());
        workspace.revision = workspace.revision.next();
        infra(self.store.compare_and_swap_selected_workspace(
            selected_id,
            record.revision,
            &RuleRepositoryAdapter::workspace_record(&workspace)?,
        ))?;
        Ok(Some(self.path.clone()))
    }

    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

fn request_delay_draft(name: &str, one_shot: bool) -> AppRuleDraft {
    AppRuleDraft {
        rule_id: None,
        expected_revision: None,
        name: name.into(),
        description: String::new(),
        enabled: true,
        priority: 10,
        channel: None,
        stage: Some(AppMessageStage::Request),
        conditions: Vec::new(),
        actions: vec![AppRuleAction::Delay { milliseconds: 10 }],
        one_shot,
    }
}

fn seed_workspace(store: &Arc<SqliteStore>, rules: Vec<Rule>) -> ProxyWorkspace {
    let workspace = ProxyWorkspace {
        rules,
        ..ProxyWorkspace::default()
    };
    store
        .insert_workspace(&RuleRepositoryAdapter::workspace_record(&workspace).expect("record"))
        .expect("seed workspace");
    workspace
}

fn adapter_with(
    store: Arc<SqliteStore>,
    dialog: Arc<dyn NativeFileDialog>,
) -> Arc<RuleRepositoryAdapter> {
    if store
        .load_workspaces()
        .expect("workspaces")
        .records
        .is_empty()
    {
        seed_workspace(&store, Vec::new());
    }
    Arc::new(RuleRepositoryAdapter::new(
        store,
        dialog,
        Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
        &[],
        &[],
    ))
}

fn adapter() -> Arc<RuleRepositoryAdapter> {
    adapter_with(
        Arc::new(SqliteStore::in_memory().expect("store")),
        Arc::new(NoDialog),
    )
}

fn runtime_snapshot(adapter: &RuleRepositoryAdapter) -> RuleRuntimeSnapshot {
    let workspace = adapter
        .load_selected_workspace()
        .expect("selected workspace");
    let channel = workspace.listeners[0].id().to_string();
    adapter
        .runtime_snapshot(&channel)
        .expect("runtime snapshot")
}

fn legacy_rule_json(rule_id: uuid::Uuid, terminal_action: &Value) -> Value {
    serde_json::json!({
        "id": rule_id,
        "revision": 3,
        "name": "legacy Shift-JIS rule",
        "description": "persisted by the pre-generic Payment proxy",
        "enabled": true,
        "priority": 10,
        "created_order": 1,
        "channel": null,
        "stage": "Request",
        "conditions": [],
        "actions": [{"Terminal": terminal_action}],
        "one_shot": false,
        "hit_count": 0,
        "last_hit_at": null
    })
}

mod concurrency;
mod conversion;
mod crud;
mod legacy_and_runtime;
