use intercept_proxy_protocol_scripting::{
    ProtocolEntryPoint, ProtocolFrameInspection, ProtocolFramingError, ProtocolRuntimeError,
};
use intercept_proxy_runtime::{
    FrameBoundary, SocketProcessingFailure, SocketProcessingFailureKind,
};

pub(super) fn frame_boundary(inspection: ProtocolFrameInspection) -> FrameBoundary {
    match inspection {
        ProtocolFrameInspection::NeedMore { total } => FrameBoundary::NeedMore { total },
        ProtocolFrameInspection::Complete { bytes } => FrameBoundary::Complete { bytes },
        ProtocolFrameInspection::Reject { reason } => FrameBoundary::Reject { reason },
    }
}

pub(super) fn framing_failure(error: &ProtocolFramingError) -> SocketProcessingFailure {
    let kind = match error {
        ProtocolFramingError::FrameTooLarge { .. }
        | ProtocolFramingError::FifoLimitExceeded { .. } => {
            SocketProcessingFailureKind::BufferLimitExceeded
        }
        ProtocolFramingError::InvalidLimit { .. }
        | ProtocolFramingError::FifoSmallerThanFrame { .. } => {
            SocketProcessingFailureKind::InvalidLimits
        }
        ProtocolFramingError::InvalidDecisionLength
        | ProtocolFramingError::InvalidRejectReason
        | ProtocolFramingError::NeedMoreWithoutProgress
        | ProtocolFramingError::CompleteEmpty
        | ProtocolFramingError::CompleteOutOfBounds => {
            SocketProcessingFailureKind::InvalidFrameBoundary
        }
        ProtocolFramingError::Rejected { .. } => SocketProcessingFailureKind::FrameRejected,
        ProtocolFramingError::TruncatedFrame { .. } => SocketProcessingFailureKind::TruncatedFrame,
        ProtocolFramingError::ReaderOutOfBounds
        | ProtocolFramingError::EmptyFindPattern
        | ProtocolFramingError::InvalidFindStart
        | ProtocolFramingError::FrameEntryFailed { .. } => {
            SocketProcessingFailureKind::ProcessingFailed
        }
        ProtocolFramingError::FrameExecutionCancelled { .. } => {
            SocketProcessingFailureKind::Cancelled
        }
    };
    SocketProcessingFailure::new(kind, "protocol frame inspection failed")
}

pub(super) fn runtime_failure(error: &ProtocolRuntimeError) -> SocketProcessingFailure {
    let kind = match error {
        ProtocolRuntimeError::ExecutionCancelled { .. } => SocketProcessingFailureKind::Cancelled,
        ProtocolRuntimeError::DocumentTransformFailed { .. } => {
            SocketProcessingFailureKind::RuleFailed
        }
        error => entry_failure_kind(error).unwrap_or(SocketProcessingFailureKind::ProcessingFailed),
    };
    SocketProcessingFailure::new(kind, "protocol frame processing failed")
}

fn entry_failure_kind(error: &ProtocolRuntimeError) -> Option<SocketProcessingFailureKind> {
    let entry = match error {
        ProtocolRuntimeError::EntryPointFailed { entry, .. }
        | ProtocolRuntimeError::ResourceLimitExceeded { entry, .. } => *entry,
        _ => return None,
    };
    Some(match entry {
        ProtocolEntryPoint::Decode => SocketProcessingFailureKind::DecodeFailed,
        ProtocolEntryPoint::Encode | ProtocolEntryPoint::Display => {
            SocketProcessingFailureKind::EncodeFailed
        }
        ProtocolEntryPoint::Frame => SocketProcessingFailureKind::ProcessingFailed,
    })
}

pub(super) fn processing_failure(message: &'static str) -> SocketProcessingFailure {
    SocketProcessingFailure::new(SocketProcessingFailureKind::ProcessingFailed, message)
}

pub(super) fn worker_failure() -> SocketProcessingFailure {
    SocketProcessingFailure::new(
        SocketProcessingFailureKind::ProcessorPanicked,
        "scripted direction worker stopped",
    )
}

pub(super) fn invalid_limits() -> SocketProcessingFailure {
    SocketProcessingFailure::new(
        SocketProcessingFailureKind::InvalidLimits,
        "scripted frame limits cannot be represented safely",
    )
}

#[cfg(test)]
mod tests {
    use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};

    use super::*;

    fn package() -> ProtocolPackageRef {
        ProtocolPackageRef {
            id: ProtocolPackageId::new("diagnostic-test").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        }
    }

    #[test]
    fn producer_maps_decode_rule_and_encode_runtime_errors() {
        for (error, expected) in [
            (
                ProtocolRuntimeError::EntryPointFailed {
                    package: package(),
                    entry: ProtocolEntryPoint::Decode,
                },
                SocketProcessingFailureKind::DecodeFailed,
            ),
            (
                ProtocolRuntimeError::DocumentTransformFailed { package: package() },
                SocketProcessingFailureKind::RuleFailed,
            ),
            (
                ProtocolRuntimeError::EntryPointFailed {
                    package: package(),
                    entry: ProtocolEntryPoint::Encode,
                },
                SocketProcessingFailureKind::EncodeFailed,
            ),
        ] {
            assert_eq!(runtime_failure(&error).kind, expected);
        }
    }
}

#[cfg(test)]
#[path = "failure/coverage_tests.rs"]
mod coverage_tests;
