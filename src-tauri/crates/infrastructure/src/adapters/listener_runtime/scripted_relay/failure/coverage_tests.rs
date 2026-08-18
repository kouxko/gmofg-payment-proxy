use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use intercept_proxy_protocol_scripting::{
    ProtocolEntryPoint, ProtocolFrameInspection, ProtocolFramingError, ProtocolFramingLimit,
    ProtocolResourceLimit, ProtocolRuntimeError,
};
use intercept_proxy_runtime::{FrameBoundary, SocketProcessingFailureKind};

use super::*;

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("relay-failure-coverage").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

#[test]
fn frame_boundary_preserves_all_decision_variants() {
    for (input, expected) in [
        (
            ProtocolFrameInspection::NeedMore { total: 3 },
            FrameBoundary::NeedMore { total: 3 },
        ),
        (
            ProtocolFrameInspection::Complete { bytes: 2 },
            FrameBoundary::Complete { bytes: 2 },
        ),
        (
            ProtocolFrameInspection::Reject {
                reason: "bad".into(),
            },
            FrameBoundary::Reject {
                reason: "bad".into(),
            },
        ),
    ] {
        assert_eq!(frame_boundary(input), expected);
    }
}

#[test]
fn framing_failure_covers_every_typed_kind_group() {
    for (error, expected) in framing_cases() {
        assert_eq!(framing_failure(&error).kind, expected);
    }
}

#[test]
fn runtime_failure_covers_cancel_entry_and_fallback_groups() {
    for (error, expected) in [
        (
            ProtocolRuntimeError::ExecutionCancelled {
                package: package(),
                entry: ProtocolEntryPoint::Decode,
            },
            SocketProcessingFailureKind::Cancelled,
        ),
        (
            ProtocolRuntimeError::DocumentTransformFailed { package: package() },
            SocketProcessingFailureKind::RuleFailed,
        ),
        (
            ProtocolRuntimeError::EntryPointFailed {
                package: package(),
                entry: ProtocolEntryPoint::Decode,
            },
            SocketProcessingFailureKind::DecodeFailed,
        ),
        (
            ProtocolRuntimeError::ResourceLimitExceeded {
                package: package(),
                entry: ProtocolEntryPoint::Display,
                limit: ProtocolResourceLimit::Operations,
            },
            SocketProcessingFailureKind::EncodeFailed,
        ),
        (
            ProtocolRuntimeError::EntryPointFailed {
                package: package(),
                entry: ProtocolEntryPoint::Frame,
            },
            SocketProcessingFailureKind::ProcessingFailed,
        ),
        (
            ProtocolRuntimeError::CompilationFailed { package: package() },
            SocketProcessingFailureKind::ProcessingFailed,
        ),
    ] {
        assert_eq!(runtime_failure(&error).kind, expected);
    }
}

#[test]
fn fixed_failure_helpers_keep_stable_kinds_and_codes() {
    for (failure, kind) in [
        (
            processing_failure("fixed"),
            SocketProcessingFailureKind::ProcessingFailed,
        ),
        (
            worker_failure(),
            SocketProcessingFailureKind::ProcessorPanicked,
        ),
        (invalid_limits(), SocketProcessingFailureKind::InvalidLimits),
    ] {
        assert_eq!(failure.kind, kind);
        assert_eq!(failure.stable_code(), kind.as_str());
    }
}

fn framing_cases() -> Vec<(ProtocolFramingError, SocketProcessingFailureKind)> {
    vec![
        (
            ProtocolFramingError::FrameTooLarge {
                frame_bytes: 2,
                maximum: 1,
            },
            SocketProcessingFailureKind::BufferLimitExceeded,
        ),
        (
            ProtocolFramingError::FifoLimitExceeded { maximum: 1 },
            SocketProcessingFailureKind::BufferLimitExceeded,
        ),
        (
            ProtocolFramingError::InvalidLimit {
                limit: ProtocolFramingLimit::FrameBytes,
                value: 0,
                maximum: 1,
            },
            SocketProcessingFailureKind::InvalidLimits,
        ),
        (
            ProtocolFramingError::FifoSmallerThanFrame {
                frame_bytes: 2,
                fifo_bytes: 1,
            },
            SocketProcessingFailureKind::InvalidLimits,
        ),
        (
            ProtocolFramingError::CompleteEmpty,
            SocketProcessingFailureKind::InvalidFrameBoundary,
        ),
        (
            ProtocolFramingError::Rejected {
                reason: "no".into(),
            },
            SocketProcessingFailureKind::FrameRejected,
        ),
        (
            ProtocolFramingError::TruncatedFrame { buffered_bytes: 1 },
            SocketProcessingFailureKind::TruncatedFrame,
        ),
        (
            ProtocolFramingError::ReaderOutOfBounds,
            SocketProcessingFailureKind::ProcessingFailed,
        ),
        (
            ProtocolFramingError::FrameExecutionCancelled { package: package() },
            SocketProcessingFailureKind::Cancelled,
        ),
    ]
}
