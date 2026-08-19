//! MCP query inputs and their normalized application DTOs.

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    CaptureQuery, CaptureSort, ChannelId, DiagnosticLogQuery, ListenerId, MessageStage,
    PageRequest, ProtocolDirection, ProtocolPackageRef, RuleId, SessionId, SocketCaptureKind,
    SocketCaptureQuery, SocketCaptureSort, SocketConnectionId, SortDirection, WorkspaceId,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiagnosticArguments {
    pub keyword: Option<String>,
    pub after_event_id: Option<u64>,
    pub limit: Option<u16>,
}

impl DiagnosticArguments {
    pub fn into_query(self) -> DiagnosticLogQuery {
        DiagnosticLogQuery {
            keyword: self.keyword,
            after_event_id: self.after_event_id,
            limit: self.limit.unwrap_or(300),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpCaptureArguments {
    pub keyword: Option<String>,
    pub terminal_ip: Option<String>,
    pub channel: Option<ChannelId>,
    pub stage: Option<MessageStage>,
    pub result: Option<String>,
    pub rule_id: Option<RuleId>,
    pub after_event_id: Option<u64>,
    pub sort: Option<CaptureSort>,
    pub direction: Option<SortDirection>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

impl HttpCaptureArguments {
    pub fn into_query(self) -> CaptureQuery {
        CaptureQuery {
            keyword: self.keyword,
            terminal_ip: self.terminal_ip,
            channel: self.channel,
            stage: self.stage,
            result: self.result,
            rule_id: self.rule_id,
            after_event_id: self.after_event_id,
            sort: self.sort.unwrap_or(CaptureSort::OccurredAt),
            direction: self.direction.unwrap_or(SortDirection::Desc),
            page: PageRequest {
                page: self.page.unwrap_or(1),
                page_size: self.page_size.unwrap_or(100),
            },
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SocketCaptureArguments {
    pub workspace_id: Option<WorkspaceId>,
    pub listener_id: Option<ListenerId>,
    pub session_id: Option<SessionId>,
    pub connection_id: Option<SocketConnectionId>,
    pub package: Option<ProtocolPackageRef>,
    pub direction: Option<ProtocolDirection>,
    pub kind: Option<SocketCaptureKind>,
    pub occurred_from: Option<DateTime<Utc>>,
    pub occurred_to: Option<DateTime<Utc>>,
    pub sort: Option<SocketCaptureSort>,
    pub direction_sort: Option<SortDirection>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

impl SocketCaptureArguments {
    pub fn into_query(self) -> SocketCaptureQuery {
        SocketCaptureQuery {
            workspace_id: self.workspace_id,
            listener_id: self.listener_id,
            session_id: self.session_id,
            connection_id: self.connection_id,
            package: self.package,
            direction: self.direction,
            kind: self.kind,
            occurred_from: self.occurred_from,
            occurred_to: self.occurred_to,
            sort: self.sort.unwrap_or(SocketCaptureSort::OccurredAt),
            direction_sort: self.direction_sort.unwrap_or(SortDirection::Desc),
            page: PageRequest {
                page: self.page.unwrap_or(1),
                page_size: self.page_size.unwrap_or(100),
            },
        }
    }
}

#[cfg(test)]
mod tests;
