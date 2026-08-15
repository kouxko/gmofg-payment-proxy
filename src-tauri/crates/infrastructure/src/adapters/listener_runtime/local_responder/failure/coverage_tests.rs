use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use intercept_proxy_protocol_scripting::{
    ProtocolEntryPoint, ProtocolFrameInspection, ProtocolFramingError, ProtocolFramingLimit,
    ProtocolResourceLimit, ProtocolRuntimeError,
};
use intercept_proxy_runtime::{FrameBoundary, SocketProcessingFailureKind};

use super::*;

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("local-failure-coverage").unwrap(),
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
fn request_and_response_runtime_failures_cover_every_group() {
    let cancelled = ProtocolRuntimeError::ExecutionCancelled {
        package: package(),
        entry: ProtocolEntryPoint::Decode,
    };
    assert_eq!(
        request_runtime_failure(&cancelled).kind,
        SocketProcessingFailureKind::Cancelled
    );
    assert_eq!(
        response_runtime_failure(&cancelled).kind,
        SocketProcessingFailureKind::Cancelled
    );
    assert_eq!(
        request_runtime_failure(&ProtocolRuntimeError::CompilationFailed { package: package() })
            .kind,
        SocketProcessingFailureKind::DecodeFailed
    );
    for (error, expected) in [
        (
            ProtocolRuntimeError::LocalResponseEmpty { package: package() },
            SocketProcessingFailureKind::EmptyOutput,
        ),
        (
            ProtocolRuntimeError::ResourceLimitExceeded {
                package: package(),
                entry: ProtocolEntryPoint::Encode,
                limit: ProtocolResourceLimit::BlobBytes,
            },
            SocketProcessingFailureKind::OutputLimitExceeded,
        ),
        (
            ProtocolRuntimeError::DocumentTransformFailed { package: package() },
            SocketProcessingFailureKind::RuleFailed,
        ),
        (
            ProtocolRuntimeError::EntryPointUnavailable {
                package: package(),
                direction: intercept_proxy_protocol_scripting::ProtocolDirection::Downstream,
                entry: ProtocolEntryPoint::Decode,
            },
            SocketProcessingFailureKind::DecodeFailed,
        ),
        (
            ProtocolRuntimeError::EntryPointFailed {
                package: package(),
                entry: ProtocolEntryPoint::Display,
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
        assert_eq!(response_runtime_failure(&error).kind, expected);
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
