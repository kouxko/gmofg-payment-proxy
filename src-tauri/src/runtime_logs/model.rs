use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApplicationLogLevel {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApplicationLogEntry {
    pub log_id: u64,
    pub occurred_at: DateTime<Utc>,
    pub level: ApplicationLogLevel,
    pub target: String,
    pub message: String,
    #[serde(default)]
    pub message_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ApplicationLogQuery {
    pub level: Option<ApplicationLogLevel>,
    pub target: Option<String>,
    pub keyword: Option<String>,
    pub occurred_from: Option<DateTime<Utc>>,
    pub occurred_to: Option<DateTime<Utc>>,
    pub before_log_id: Option<u64>,
    pub limit: u16,
}

impl Default for ApplicationLogQuery {
    fn default() -> Self {
        Self {
            level: None,
            target: None,
            keyword: None,
            occurred_from: None,
            occurred_to: None,
            before_log_id: None,
            limit: 200,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ApplicationLogPage {
    pub rows: Vec<ApplicationLogEntry>,
    pub oldest_retained_log_id: Option<u64>,
    pub newest_retained_log_id: Option<u64>,
    pub evicted_count: u64,
    /// Runtime-log producer messages rejected because the count or shared byte budget was full.
    pub queue_dropped_full: u64,
    /// Runtime-log producer messages rejected after the owned consumer was disconnected.
    pub queue_dropped_disconnected: u64,
    /// Runtime-log producer messages rejected instead of waiting for the sender lock.
    pub queue_dropped_contended: u64,
    pub corrupt_line_count: u64,
    pub has_more: bool,
    pub persistence_error: Option<String>,
    pub storage_path: Option<String>,
    pub retained_capacity: usize,
    pub max_file_bytes: u64,
}
