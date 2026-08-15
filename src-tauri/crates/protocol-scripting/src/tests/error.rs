use std::error::Error;

use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};

use crate::{ProtocolDirection, ProtocolEntryPoint, ProtocolResourceLimit, ProtocolRuntimeError};

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583-standard").unwrap(),
        version: ProtocolPackageVersion::new("1.2.3").unwrap(),
    }
}

#[test]
fn runtime_error_codes_display_and_source_behavior_are_stable() {
    let cases = [
        (
            ProtocolRuntimeError::InvalidResourceLimit {
                limit: ProtocolResourceLimit::Operations,
                value: 0,
                maximum: 10,
            },
            "INVALID_RESOURCE_LIMIT",
            "协议脚本资源限制 operations 的值 0 无效；允许范围为 1..=10",
        ),
        (
            ProtocolRuntimeError::CompilationFailed { package: package() },
            "COMPILATION_FAILED",
            "协议包 iso8583-standard@1.2.3 编译失败",
        ),
        (
            ProtocolRuntimeError::EntryPointUnavailable {
                package: package(),
                direction: ProtocolDirection::Downstream,
                entry: ProtocolEntryPoint::Encode,
            },
            "ENTRY_POINT_UNAVAILABLE",
            "协议包 iso8583-standard@1.2.3 的 downstream 方向未声明 encode 入口",
        ),
        (
            ProtocolRuntimeError::EntryPointFailed {
                package: package(),
                entry: ProtocolEntryPoint::Decode,
            },
            "ENTRY_POINT_FAILED",
            "协议包 iso8583-standard@1.2.3 的 decode 入口执行失败",
        ),
        (
            ProtocolRuntimeError::DocumentTransformFailed { package: package() },
            "DOCUMENT_TRANSFORM_FAILED",
            "协议包 iso8583-standard@1.2.3 的 Document 变换失败",
        ),
        (
            ProtocolRuntimeError::ExecutionCancelled {
                package: package(),
                entry: ProtocolEntryPoint::Decode,
            },
            "EXECUTION_CANCELLED",
            "协议包 iso8583-standard@1.2.3 的 decode 入口执行已取消",
        ),
        (
            ProtocolRuntimeError::ResourceLimitExceeded {
                package: package(),
                entry: ProtocolEntryPoint::Encode,
                limit: ProtocolResourceLimit::BlobBytes,
            },
            "RESOURCE_LIMIT_EXCEEDED",
            "协议包 iso8583-standard@1.2.3 的 encode 入口超过 blob_bytes 限制",
        ),
    ];

    for (error, code, display) in cases {
        assert_eq!(error.code(), code);
        assert_eq!(error.to_string(), display);
        // 可序列化边界错误不携带第三方 source，避免源码、路径或引擎内部消息跨层泄漏。
        assert!(error.source().is_none());
    }
}

#[test]
fn runtime_errors_have_strict_unambiguous_serde_contracts() {
    let cases = [
        ProtocolRuntimeError::InvalidResourceLimit {
            limit: ProtocolResourceLimit::WallTimeMs,
            value: 30_001,
            maximum: 30_000,
        },
        ProtocolRuntimeError::CompilationFailed { package: package() },
        ProtocolRuntimeError::EntryPointUnavailable {
            package: package(),
            direction: ProtocolDirection::Upstream,
            entry: ProtocolEntryPoint::Encode,
        },
        ProtocolRuntimeError::EntryPointFailed {
            package: package(),
            entry: ProtocolEntryPoint::Frame,
        },
        ProtocolRuntimeError::DocumentTransformFailed { package: package() },
        ProtocolRuntimeError::ExecutionCancelled {
            package: package(),
            entry: ProtocolEntryPoint::Display,
        },
        ProtocolRuntimeError::ResourceLimitExceeded {
            package: package(),
            entry: ProtocolEntryPoint::Display,
            limit: ProtocolResourceLimit::StringBytes,
        },
    ];

    for error in cases {
        let value = serde_json::to_value(&error).unwrap();
        assert_eq!(value["code"], error.code());
        assert_eq!(
            serde_json::from_value::<ProtocolRuntimeError>(value).unwrap(),
            error
        );
    }

    let mut unknown =
        serde_json::to_value(ProtocolRuntimeError::CompilationFailed { package: package() })
            .unwrap();
    unknown["source"] = serde_json::json!("raw third-party error");
    assert!(serde_json::from_value::<ProtocolRuntimeError>(unknown).is_err());
    assert!(
        serde_json::from_value::<ProtocolRuntimeError>(
            serde_json::json!({"code": "UNKNOWN_RUNTIME_FAILURE"})
        )
        .is_err()
    );
}

#[test]
fn entry_points_and_resource_limits_cover_every_wire_value() {
    for (entry, wire) in [
        (ProtocolEntryPoint::Frame, "frame"),
        (ProtocolEntryPoint::Decode, "decode"),
        (ProtocolEntryPoint::Encode, "encode"),
        (ProtocolEntryPoint::Display, "display"),
    ] {
        assert_eq!(entry.to_string(), wire);
        assert_eq!(serde_json::to_value(entry).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<ProtocolEntryPoint>(wire.into()).unwrap(),
            entry
        );
    }
    for (limit, wire) in [
        (ProtocolResourceLimit::Operations, "operations"),
        (ProtocolResourceLimit::CallDepth, "call_depth"),
        (ProtocolResourceLimit::StringBytes, "string_bytes"),
        (ProtocolResourceLimit::BlobBytes, "blob_bytes"),
        (ProtocolResourceLimit::WallTimeMs, "wall_time_ms"),
    ] {
        assert_eq!(limit.to_string(), wire);
        assert_eq!(serde_json::to_value(limit).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<ProtocolResourceLimit>(wire.into()).unwrap(),
            limit
        );
    }
    assert!(serde_json::from_value::<ProtocolEntryPoint>("send".into()).is_err());
    assert!(serde_json::from_value::<ProtocolResourceLimit>("unbounded".into()).is_err());
}
