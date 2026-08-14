use rhai::{Engine, EvalAltResult, INT, ImmutableString, Module, Position};

use super::{ProtocolFramingError, ProtocolFramingResult};

const MAX_REJECT_REASON_BYTES: usize = 512;

/// `frame()` 返回给宿主的唯一合法结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FramingDecision {
    /// 当前 Frame 至少需要 FIFO 达到该总字节数。
    NeedMore(usize),
    /// FIFO 前若干字节构成完整 Frame。
    Complete(usize),
    /// 当前字节流不属于本协议或已经无法继续解析。
    Reject(String),
}

pub(super) fn register(engine: &mut Engine) {
    let mut module = Module::new();
    module.set_native_fn("need_more", |total: INT| {
        decision_length(total).map(FramingDecision::NeedMore)
    });
    module.set_native_fn("complete", |length: INT| {
        decision_length(length).map(FramingDecision::Complete)
    });
    module.set_native_fn("reject", |reason: ImmutableString| {
        reject_reason(reason.as_str()).map(FramingDecision::Reject)
    });
    engine
        .register_type_with_name::<FramingDecision>("FramingDecision")
        .register_static_module("framing", module.into());
}

fn decision_length(value: INT) -> Result<usize, Box<EvalAltResult>> {
    usize::try_from(value)
        .map_err(|_| Box::new(host_error(&ProtocolFramingError::InvalidDecisionLength)))
}

fn reject_reason(reason: &str) -> Result<String, Box<EvalAltResult>> {
    if reason.is_empty() || reason.len() > MAX_REJECT_REASON_BYTES {
        Err(Box::new(host_error(
            &ProtocolFramingError::InvalidRejectReason,
        )))
    } else {
        Ok(reason.to_owned())
    }
}

fn host_error(error: &ProtocolFramingError) -> EvalAltResult {
    EvalAltResult::ErrorRuntime(format!("{:?}", error.code()).into(), Position::NONE)
}

pub(super) fn validate_decision(
    decision: FramingDecision,
    available: usize,
    max_frame_bytes: usize,
) -> ProtocolFramingResult<FramingDecision> {
    match decision {
        FramingDecision::NeedMore(total) if total <= available => {
            Err(ProtocolFramingError::NeedMoreWithoutProgress)
        }
        FramingDecision::NeedMore(total) if total > max_frame_bytes => {
            Err(ProtocolFramingError::FrameTooLarge {
                frame_bytes: usize_to_u64(total),
                maximum: usize_to_u64(max_frame_bytes),
            })
        }
        FramingDecision::Complete(0) => Err(ProtocolFramingError::CompleteEmpty),
        FramingDecision::Complete(length) if length > available => {
            Err(ProtocolFramingError::CompleteOutOfBounds)
        }
        FramingDecision::Complete(length) if length > max_frame_bytes => {
            Err(ProtocolFramingError::FrameTooLarge {
                frame_bytes: usize_to_u64(length),
                maximum: usize_to_u64(max_frame_bytes),
            })
        }
        FramingDecision::Reject(reason)
            if reason.is_empty() || reason.len() > MAX_REJECT_REASON_BYTES =>
        {
            Err(ProtocolFramingError::InvalidRejectReason)
        }
        decision => Ok(decision),
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
