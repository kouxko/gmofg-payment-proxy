use intercept_proxy_application::{HttpProtocolFailureKind, HttpProtocolFailureViewModel};
use intercept_proxy_domain::ProtocolRuleStage;
use intercept_proxy_protocol_scripting::{
    ProtocolDirection, ProtocolEntryPoint, ProtocolRuntimeError,
};
use intercept_proxy_runtime::{ErrorCode, ProxyError};

pub(super) struct HttpProtocolProcessError {
    pub(super) failure: HttpProtocolFailureViewModel,
    pub(super) error: ProxyError,
}

fn protocol_error(
    error: &ProtocolRuntimeError,
    direction: ProtocolDirection,
    stage: Option<ProtocolRuleStage>,
) -> ProxyError {
    let direction_text = match direction {
        ProtocolDirection::Upstream => "上行(应用→代理→上游)",
        ProtocolDirection::Downstream => "下行(上游→代理→应用)",
    };
    ProxyError::new(
        ErrorCode::Internal,
        format!(
            "HTTP 协议处理失败[{direction_text}{}]：{error} ({})",
            stage.map_or_else(String::new, |value| format!("/{value:?}")),
            error.code()
        ),
    )
}

pub(super) fn failure_view(
    package: intercept_proxy_domain::ProtocolPackageRef,
    direction: ProtocolDirection,
    stage: Option<ProtocolRuleStage>,
    kind: HttpProtocolFailureKind,
    code: impl Into<String>,
    detail: impl Into<String>,
    origin_body: Vec<u8>,
) -> HttpProtocolFailureViewModel {
    HttpProtocolFailureViewModel {
        package,
        direction: match direction {
            ProtocolDirection::Upstream => intercept_proxy_domain::ProtocolDirection::Upstream,
            ProtocolDirection::Downstream => intercept_proxy_domain::ProtocolDirection::Downstream,
        },
        stage,
        kind,
        code: code.into(),
        detail: detail.into(),
        origin_body,
    }
}

const fn runtime_failure_kind(error: &ProtocolRuntimeError) -> HttpProtocolFailureKind {
    match error {
        ProtocolRuntimeError::DocumentTransformFailed { .. } => HttpProtocolFailureKind::RuleFailed,
        ProtocolRuntimeError::EntryPointFailed { entry, .. }
        | ProtocolRuntimeError::ExecutionCancelled { entry, .. }
        | ProtocolRuntimeError::ResourceLimitExceeded { entry, .. } => match entry {
            ProtocolEntryPoint::Decode => HttpProtocolFailureKind::DecodeFailed,
            ProtocolEntryPoint::Encode => HttpProtocolFailureKind::EncodeFailed,
            ProtocolEntryPoint::Frame | ProtocolEntryPoint::Display => {
                HttpProtocolFailureKind::WorkerFailed
            }
        },
        _ => HttpProtocolFailureKind::WorkerFailed,
    }
}

const fn failure_detail(kind: HttpProtocolFailureKind) -> &'static str {
    match kind {
        HttpProtocolFailureKind::InputNotUtf8 => "HTTP Body 不是 UTF-8 文本",
        HttpProtocolFailureKind::DecodeFailed => "协议包 Decode 失败",
        HttpProtocolFailureKind::RuleFailed => "协议报文规则执行失败",
        HttpProtocolFailureKind::EncodeFailed => "协议包 Encode 失败",
        HttpProtocolFailureKind::OutputNotUtf8 => "协议包 Encode 输出不是 UTF-8 文本",
        HttpProtocolFailureKind::WorkerFailed => "HTTP 协议处理任务失败",
    }
}

pub(super) fn runtime_process_error(
    package: intercept_proxy_domain::ProtocolPackageRef,
    direction: ProtocolDirection,
    origin_body: Vec<u8>,
    error: &ProtocolRuntimeError,
    stage: Option<ProtocolRuleStage>,
) -> HttpProtocolProcessError {
    let kind = runtime_failure_kind(error);
    let code = error.code();
    HttpProtocolProcessError {
        failure: failure_view(
            package,
            direction,
            stage,
            kind,
            code,
            failure_detail(kind),
            origin_body,
        ),
        error: protocol_error(error, direction, stage),
    }
}

#[cfg(test)]
mod failure_stage_tests {
    use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};

    use super::*;

    #[test]
    fn every_http_rule_stage_keeps_exact_failure_stage_and_stable_evidence() {
        let package = intercept_proxy_domain::ProtocolPackageRef {
            id: ProtocolPackageId::new("http-failure-stage-test").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        };
        for stage in [
            ProtocolRuleStage::AppToProxy,
            ProtocolRuleStage::ProxyToUpstream,
            ProtocolRuleStage::UpstreamToProxy,
            ProtocolRuleStage::ProxyToApp,
        ] {
            let direction = match stage {
                ProtocolRuleStage::AppToProxy | ProtocolRuleStage::ProxyToUpstream => {
                    ProtocolDirection::Upstream
                }
                ProtocolRuleStage::UpstreamToProxy | ProtocolRuleStage::ProxyToApp => {
                    ProtocolDirection::Downstream
                }
            };
            let result = runtime_process_error(
                package.clone(),
                direction,
                b"exact-origin".to_vec(),
                &ProtocolRuntimeError::DocumentTransformFailed {
                    package: package.clone(),
                },
                Some(stage),
            );
            assert_eq!(result.failure.stage, Some(stage));
            assert_eq!(result.failure.kind, HttpProtocolFailureKind::RuleFailed);
            assert_eq!(result.failure.code, "DOCUMENT_TRANSFORM_FAILED");
            assert_eq!(result.failure.origin_body, b"exact-origin");
        }
    }
}
