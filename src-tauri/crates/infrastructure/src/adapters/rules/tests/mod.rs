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
        let mut rules = workspace.http_runtime_rules()?;
        rules.push(self.concurrent_rule.clone());
        workspace.replace_http_runtime_rules(rules)?;
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
        channel: Some(test_channel()),
        stage: Some(AppMessageStage::Request),
        conditions: Vec::new(),
        actions: vec![AppRuleAction::Delay { milliseconds: 10 }],
        one_shot,
    }
}

fn test_listener_id() -> intercept_proxy_domain::ListenerId {
    intercept_proxy_domain::ListenerId::from_uuid(
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000123").expect("listener UUID"),
    )
}

fn test_channel() -> intercept_proxy_domain::ChannelId {
    intercept_proxy_domain::ChannelId::new(test_listener_id().to_string()).expect("channel")
}

fn seed_workspace(store: &Arc<SqliteStore>, rules: Vec<Rule>) -> ProxyWorkspace {
    let first_workspace = store
        .load_workspaces()
        .expect("workspaces")
        .records
        .is_empty();
    let mut workspace = ProxyWorkspace::default();
    if first_workspace {
        workspace.listeners[0].id = test_listener_id();
    }
    let channel = ChannelId::new(workspace.listeners[0].id.to_string()).expect("channel");
    let mut rules = rules;
    for rule in &mut rules {
        rule.channel = Some(channel.clone());
    }
    workspace.replace_http_runtime_rules(rules).expect("rules");
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
    Arc::new(RuleRepositoryAdapter::new(store, dialog, &[]))
}

fn adapter() -> Arc<RuleRepositoryAdapter> {
    adapter_with(
        Arc::new(SqliteStore::in_memory().expect("store")),
        Arc::new(NoDialog),
    )
}

async fn runtime_snapshot(adapter: &RuleRepositoryAdapter) -> RuleRuntimeSnapshot {
    let workspace = adapter
        .load_selected_workspace()
        .expect("selected workspace");
    let channel = workspace.listeners[0].id().to_string();
    adapter
        .runtime_snapshot(&channel)
        .await
        .expect("runtime snapshot")
}

mod concurrency;
mod conversion;
mod crud;
mod legacy_and_runtime;
