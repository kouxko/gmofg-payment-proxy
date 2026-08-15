use intercept_proxy_protocol_scripting::{
    ProtocolEntryPoint, ProtocolFrameInspection, ProtocolFramingError, ProtocolResourceLimit,
    ProtocolRuntimeError,
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
    SocketProcessingFailure::new(kind, "local request frame inspection failed")
}

pub(super) fn request_runtime_failure(error: &ProtocolRuntimeError) -> SocketProcessingFailure {
    let kind = if cancelled(error) {
        SocketProcessingFailureKind::Cancelled
    } else {
        // LocalResponder request processing is exclusively the request Decode side.
        SocketProcessingFailureKind::DecodeFailed
    };
    SocketProcessingFailure::new(kind, "local request processing failed")
}

pub(super) fn response_runtime_failure(error: &ProtocolRuntimeError) -> SocketProcessingFailure {
    let kind = match error {
        error if cancelled(error) => SocketProcessingFailureKind::Cancelled,
        ProtocolRuntimeError::LocalResponseEmpty { .. } => SocketProcessingFailureKind::EmptyOutput,
        ProtocolRuntimeError::ResourceLimitExceeded {
            limit: ProtocolResourceLimit::BlobBytes,
            ..
        } => SocketProcessingFailureKind::OutputLimitExceeded,
        ProtocolRuntimeError::DocumentTransformFailed { .. } => {
            SocketProcessingFailureKind::RuleFailed
        }
        error => entry_failure_kind(error).unwrap_or(SocketProcessingFailureKind::ProcessingFailed),
    };
    SocketProcessingFailure::new(kind, "local request-response processing failed")
}

fn cancelled(error: &ProtocolRuntimeError) -> bool {
    matches!(
        error,
        ProtocolRuntimeError::ExecutionCancelled { .. }
            | ProtocolRuntimeError::LocalResponseCancelled { .. }
    )
}

fn entry_failure_kind(error: &ProtocolRuntimeError) -> Option<SocketProcessingFailureKind> {
    let entry = match error {
        ProtocolRuntimeError::EntryPointUnavailable { entry, .. }
        | ProtocolRuntimeError::EntryPointFailed { entry, .. }
        | ProtocolRuntimeError::ExecutionCancelled { entry, .. }
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
        "local responder worker stopped",
    )
}

pub(super) fn invalid_limits() -> SocketProcessingFailure {
    SocketProcessingFailure::new(
        SocketProcessingFailureKind::InvalidLimits,
        "local responder frame limits cannot be represented safely",
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
    fn producer_maps_request_and_response_phases_without_dynamic_text() {
        let decode = request_runtime_failure(&ProtocolRuntimeError::EntryPointFailed {
            package: package(),
            entry: ProtocolEntryPoint::Decode,
        });
        assert_eq!(decode.kind, SocketProcessingFailureKind::DecodeFailed);

        let rule = response_runtime_failure(&ProtocolRuntimeError::DocumentTransformFailed {
            package: package(),
        });
        assert_eq!(rule.kind, SocketProcessingFailureKind::RuleFailed);

        let encode = response_runtime_failure(&ProtocolRuntimeError::EntryPointFailed {
            package: package(),
            entry: ProtocolEntryPoint::Encode,
        });
        assert_eq!(encode.kind, SocketProcessingFailureKind::EncodeFailed);
    }
}

#[cfg(test)]
#[path = "failure/coverage_tests.rs"]
mod coverage_tests;
