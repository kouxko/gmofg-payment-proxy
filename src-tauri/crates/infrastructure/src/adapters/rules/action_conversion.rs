use super::{
    AppRuleAction, AppRuleDropResponseMode, AppRuleJitterScope, AppRuleTerminalAction,
    AppRuleTrafficDirection, DropResponseMode, JitterScope, RuleAction, TerminalAction,
    TrafficDirection,
};

pub(crate) fn action_to_domain(action: &AppRuleAction) -> Result<RuleAction, serde_json::Error> {
    Ok(match action {
        AppRuleAction::SetJsonField { path, value_json } => RuleAction::SetJsonField {
            path: path.clone(),
            value: serde_json::from_str(value_json)?,
        },
        AppRuleAction::ReplaceBodyText { text } => RuleAction::ReplaceBodyText(text.clone()),
        AppRuleAction::SetHeader { name, value } => RuleAction::SetHeader {
            name: name.clone(),
            value: value.clone(),
        },
        AppRuleAction::Delay { milliseconds } => RuleAction::Delay {
            milliseconds: *milliseconds,
        },
        AppRuleAction::Jitter {
            minimum_milliseconds,
            maximum_milliseconds,
            scope,
        } => RuleAction::Jitter {
            minimum_milliseconds: *minimum_milliseconds,
            maximum_milliseconds: *maximum_milliseconds,
            scope: match scope {
                AppRuleJitterScope::BeforeMessage => JitterScope::BeforeMessage,
                AppRuleJitterScope::PerChunk => JitterScope::PerChunk,
            },
        },
        AppRuleAction::Throttle {
            bytes_per_second,
            chunk_bytes,
            direction,
        } => RuleAction::Throttle {
            bytes_per_second: *bytes_per_second,
            chunk_bytes: *chunk_bytes,
            direction: traffic_direction_to_domain(*direction),
        },
        AppRuleAction::Intermittent {
            available_milliseconds,
            blocked_milliseconds,
            direction,
        } => RuleAction::Intermittent {
            available_milliseconds: *available_milliseconds,
            blocked_milliseconds: *blocked_milliseconds,
            direction: traffic_direction_to_domain(*direction),
        },
        AppRuleAction::Pause => RuleAction::Pause,
        AppRuleAction::CustomHttpStatus { status } => {
            RuleAction::CustomHttpStatus { status: *status }
        }
        AppRuleAction::Terminal { action } => {
            RuleAction::Terminal(terminal_action_to_domain(action))
        }
    })
}

fn terminal_action_to_domain(action: &AppRuleTerminalAction) -> TerminalAction {
    match action {
        AppRuleTerminalAction::RejectTlsHandshake => TerminalAction::RejectTlsHandshake,
        AppRuleTerminalAction::DisconnectBeforeUpstream => TerminalAction::DisconnectBeforeUpstream,
        AppRuleTerminalAction::UpstreamConnectTimeout { milliseconds } => {
            TerminalAction::UpstreamConnectTimeout {
                milliseconds: *milliseconds,
            }
        }
        AppRuleTerminalAction::UpstreamWriteTimeout { milliseconds } => {
            TerminalAction::UpstreamWriteTimeout {
                milliseconds: *milliseconds,
            }
        }
        AppRuleTerminalAction::UpstreamReadTimeout { milliseconds } => {
            TerminalAction::UpstreamReadTimeout {
                milliseconds: *milliseconds,
            }
        }
        AppRuleTerminalAction::DropUpstreamResponse { mode } => {
            TerminalAction::DropUpstreamResponse {
                mode: match mode {
                    AppRuleDropResponseMode::ReadCompleteResponse => {
                        DropResponseMode::ReadCompleteResponse
                    }
                    AppRuleDropResponseMode::CloseAfterRequestWrite => {
                        DropResponseMode::CloseAfterRequestWrite
                    }
                },
            }
        }
        AppRuleTerminalAction::MockResponse {
            status,
            headers,
            body_bytes,
        } => TerminalAction::MockResponse {
            status: *status,
            headers: headers.clone(),
            body_bytes: body_bytes.clone(),
        },
        AppRuleTerminalAction::InvalidJson { body_bytes } => TerminalAction::InvalidJson {
            body_bytes: body_bytes.clone(),
        },
        AppRuleTerminalAction::IncorrectContentLength { delta } => {
            TerminalAction::IncorrectContentLength { delta: *delta }
        }
        AppRuleTerminalAction::TruncateResponse { bytes } => {
            TerminalAction::TruncateResponse { bytes: *bytes }
        }
        AppRuleTerminalAction::DisconnectDuringUpstreamWrite { after_bytes } => {
            TerminalAction::DisconnectDuringUpstreamWrite {
                after_bytes: *after_bytes,
            }
        }
        AppRuleTerminalAction::DisconnectDuringDownstreamWrite { after_bytes } => {
            TerminalAction::DisconnectDuringDownstreamWrite {
                after_bytes: *after_bytes,
            }
        }
    }
}

pub(crate) fn action_to_app(action: &RuleAction) -> Result<AppRuleAction, serde_json::Error> {
    Ok(match action {
        RuleAction::SetJsonField { path, value } => AppRuleAction::SetJsonField {
            path: path.clone(),
            value_json: serde_json::to_string(value)?,
        },
        RuleAction::ReplaceBodyText(text) => AppRuleAction::ReplaceBodyText { text: text.clone() },
        RuleAction::SetHeader { name, value } => AppRuleAction::SetHeader {
            name: name.clone(),
            value: value.clone(),
        },
        RuleAction::Delay { milliseconds } => AppRuleAction::Delay {
            milliseconds: *milliseconds,
        },
        RuleAction::Jitter {
            minimum_milliseconds,
            maximum_milliseconds,
            scope,
        } => AppRuleAction::Jitter {
            minimum_milliseconds: *minimum_milliseconds,
            maximum_milliseconds: *maximum_milliseconds,
            scope: match scope {
                JitterScope::BeforeMessage => AppRuleJitterScope::BeforeMessage,
                JitterScope::PerChunk => AppRuleJitterScope::PerChunk,
            },
        },
        RuleAction::Throttle {
            bytes_per_second,
            chunk_bytes,
            direction,
        } => AppRuleAction::Throttle {
            bytes_per_second: *bytes_per_second,
            chunk_bytes: *chunk_bytes,
            direction: traffic_direction_to_app(*direction),
        },
        RuleAction::Intermittent {
            available_milliseconds,
            blocked_milliseconds,
            direction,
        } => AppRuleAction::Intermittent {
            available_milliseconds: *available_milliseconds,
            blocked_milliseconds: *blocked_milliseconds,
            direction: traffic_direction_to_app(*direction),
        },
        RuleAction::Pause => AppRuleAction::Pause,
        RuleAction::CustomHttpStatus { status } => {
            AppRuleAction::CustomHttpStatus { status: *status }
        }
        RuleAction::Terminal(action) => AppRuleAction::Terminal {
            action: terminal_action_to_app(action),
        },
    })
}

fn terminal_action_to_app(action: &TerminalAction) -> AppRuleTerminalAction {
    match action {
        TerminalAction::RejectTlsHandshake => AppRuleTerminalAction::RejectTlsHandshake,
        TerminalAction::DisconnectBeforeUpstream => AppRuleTerminalAction::DisconnectBeforeUpstream,
        TerminalAction::UpstreamConnectTimeout { milliseconds } => {
            AppRuleTerminalAction::UpstreamConnectTimeout {
                milliseconds: *milliseconds,
            }
        }
        TerminalAction::UpstreamWriteTimeout { milliseconds } => {
            AppRuleTerminalAction::UpstreamWriteTimeout {
                milliseconds: *milliseconds,
            }
        }
        TerminalAction::UpstreamReadTimeout { milliseconds } => {
            AppRuleTerminalAction::UpstreamReadTimeout {
                milliseconds: *milliseconds,
            }
        }
        TerminalAction::DropUpstreamResponse { mode } => {
            AppRuleTerminalAction::DropUpstreamResponse {
                mode: match mode {
                    DropResponseMode::ReadCompleteResponse => {
                        AppRuleDropResponseMode::ReadCompleteResponse
                    }
                    DropResponseMode::CloseAfterRequestWrite => {
                        AppRuleDropResponseMode::CloseAfterRequestWrite
                    }
                },
            }
        }
        TerminalAction::MockResponse {
            status,
            headers,
            body_bytes,
        } => AppRuleTerminalAction::MockResponse {
            status: *status,
            headers: headers.clone(),
            body_bytes: body_bytes.clone(),
        },
        TerminalAction::InvalidJson { body_bytes } => AppRuleTerminalAction::InvalidJson {
            body_bytes: body_bytes.clone(),
        },
        TerminalAction::IncorrectContentLength { delta } => {
            AppRuleTerminalAction::IncorrectContentLength { delta: *delta }
        }
        TerminalAction::TruncateResponse { bytes } => {
            AppRuleTerminalAction::TruncateResponse { bytes: *bytes }
        }
        TerminalAction::DisconnectDuringUpstreamWrite { after_bytes } => {
            AppRuleTerminalAction::DisconnectDuringUpstreamWrite {
                after_bytes: *after_bytes,
            }
        }
        TerminalAction::DisconnectDuringDownstreamWrite { after_bytes } => {
            AppRuleTerminalAction::DisconnectDuringDownstreamWrite {
                after_bytes: *after_bytes,
            }
        }
    }
}

const fn traffic_direction_to_domain(direction: AppRuleTrafficDirection) -> TrafficDirection {
    match direction {
        AppRuleTrafficDirection::Upstream => TrafficDirection::Upstream,
        AppRuleTrafficDirection::Downstream => TrafficDirection::Downstream,
    }
}

const fn traffic_direction_to_app(direction: TrafficDirection) -> AppRuleTrafficDirection {
    match direction {
        TrafficDirection::Upstream => AppRuleTrafficDirection::Upstream,
        TrafficDirection::Downstream => AppRuleTrafficDirection::Downstream,
    }
}
