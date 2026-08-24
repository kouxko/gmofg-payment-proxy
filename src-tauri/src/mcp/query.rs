//! MCP query inputs and their normalized application DTOs.

use intercept_proxy_application::{
    CaptureQuery, CaptureSort, ChannelId, DiagnosticLogQuery, MessageStage, PageRequest, RuleId,
    SortDirection,
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

#[cfg(test)]
mod tests;
