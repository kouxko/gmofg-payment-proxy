//! HTTP 文本 Body 的协议包处理管线。

use std::{fmt, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use intercept_proxy_application::{
    AppError, AppResult, HttpProtocolBodyViewModel, HttpProtocolDisplayFallbackReason,
    HttpProtocolDisplayViewModel, HttpProtocolFailureKind, HttpProtocolFailureViewModel,
    HttpProtocolRuleStageViewModel,
};
use intercept_proxy_domain::{
    HttpBodyProcessing, ProtocolRuleStage, ProxyListener, ProxyWorkspace,
};
use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, DisplayFallbackReason, ProtocolDirection, ProtocolDirectionExecutor,
    ProtocolDisplayResult, ProtocolFrameOutput, ProtocolPackageKind, ProtocolRuntimeError,
};
use intercept_proxy_runtime::{
    ChannelId, ConnectionContext, ErrorCode, FaultAction, HandshakePolicy, Message, PipelinePorts,
    ProxyError, Result as ProxyResult, TlsPeerIdentity, UpstreamSecurityEvidence,
};
use parking_lot::RwLock;
use uuid::Uuid;

use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

use super::ListenerRuntimeAdapter;

mod failure;
use failure::{HttpProtocolProcessError, failure_view, runtime_process_error};
mod programs;
use programs::{HttpDocumentRulePrograms, compile_programs};

#[derive(Clone)]
pub(super) struct HttpProtocolRuntimeSnapshot {
    package: RuntimeProtocolPackageSnapshot,
    programs: Arc<RwLock<HttpDocumentRulePrograms>>,
    listener_id: intercept_proxy_domain::ListenerId,
}

impl fmt::Debug for HttpProtocolRuntimeSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpProtocolRuntimeSnapshot")
            .field("package", self.package.compiled().package())
            .field("listener_id", &self.listener_id)
            .finish_non_exhaustive()
    }
}

struct HttpProtocolPipeline {
    inner: Arc<dyn PipelinePorts>,
    observations: Arc<dyn HttpProtocolObservationSink>,
    snapshot: Arc<HttpProtocolRuntimeSnapshot>,
}

struct HttpProtocolExecutionInput {
    package: RuntimeProtocolPackageSnapshot,
    programs: HttpDocumentRulePrograms,
    origin: Vec<u8>,
    direction: ProtocolDirection,
    first: ProtocolRuleStage,
    second: ProtocolRuleStage,
    connection_id: String,
    listener_id: String,
}

struct HttpProtocolExecutionOutput {
    frame: ProtocolFrameOutput,
    stages: Vec<HttpProtocolRuleStageViewModel>,
    display: ProtocolDisplayResult,
}

type HttpProtocolExecutionError = (ProtocolRuntimeError, Option<ProtocolRuleStage>);

/// HTTP 协议处理完成后，把最终线上 Body 与 Document/Display 证据写入同一会话。
pub trait HttpProtocolObservationSink: fmt::Debug + Send + Sync {
    fn record_http_protocol_observation(
        &self,
        context: &ConnectionContext,
        direction: ProtocolDirection,
        message: &Message,
        observation: HttpProtocolBodyViewModel,
    ) -> ProxyResult<()>;

    fn record_http_protocol_failure(
        &self,
        context: &ConnectionContext,
        message: &Message,
        failure: HttpProtocolFailureViewModel,
    ) -> ProxyResult<()>;
}

impl HttpProtocolObservationSink for intercept_proxy_runtime::NoopPipelinePorts {
    fn record_http_protocol_observation(
        &self,
        _context: &ConnectionContext,
        _direction: ProtocolDirection,
        _message: &Message,
        _observation: HttpProtocolBodyViewModel,
    ) -> ProxyResult<()> {
        Ok(())
    }

    fn record_http_protocol_failure(
        &self,
        _context: &ConnectionContext,
        _message: &Message,
        _failure: HttpProtocolFailureViewModel,
    ) -> ProxyResult<()> {
        Ok(())
    }
}

impl HttpProtocolRuntimeSnapshot {
    pub(super) fn prepare(
        adapter: &ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<Option<Arc<Self>>> {
        let intercept_proxy_domain::ListenerDataPlane::Http(http) = &listener.data_plane else {
            return Ok(None);
        };
        let HttpBodyProcessing::Protocol { package } = &http.body_processing else {
            return Ok(None);
        };
        let frozen = adapter
            .protocol_packages
            .freeze_for_listener_start(package)?;
        if frozen.compiled().kind() != ProtocolPackageKind::Http {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_KIND_MISMATCH",
                "HTTP Body 必须绑定 HTTP 协议包。",
            )
            .entity(listener.id.to_string()));
        }
        let programs = compile_programs(
            workspace,
            listener,
            package,
            frozen.compiled().schema(ProtocolDirection::Upstream),
            frozen.compiled().schema(ProtocolDirection::Downstream),
        )?;
        Ok(Some(Arc::new(Self {
            package: frozen,
            programs: Arc::new(RwLock::new(programs)),
            listener_id: listener.id,
        })))
    }

    pub(super) fn wrap(
        self: &Arc<Self>,
        inner: Arc<dyn PipelinePorts>,
        observations: Arc<dyn HttpProtocolObservationSink>,
    ) -> Arc<dyn PipelinePorts> {
        Arc::new(HttpProtocolPipeline {
            inner,
            observations,
            snapshot: Arc::clone(self),
        })
    }

    pub(super) fn replace_document_rules(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<()> {
        let intercept_proxy_domain::ListenerDataPlane::Http(http) = &listener.data_plane else {
            return Ok(());
        };
        let HttpBodyProcessing::Protocol { package } = &http.body_processing else {
            return Ok(());
        };
        let replacement = compile_programs(
            workspace,
            listener,
            package,
            self.package.compiled().schema(ProtocolDirection::Upstream),
            self.package
                .compiled()
                .schema(ProtocolDirection::Downstream),
        )?;
        *self.programs.write() = replacement;
        Ok(())
    }

    async fn process(
        &self,
        context: &ConnectionContext,
        message: &mut Message,
        direction: ProtocolDirection,
        first: ProtocolRuleStage,
        second: ProtocolRuleStage,
    ) -> Result<Option<HttpProtocolBodyViewModel>, HttpProtocolProcessError> {
        if message.body.is_empty() {
            return Ok(None);
        }
        let origin = message.body.to_vec();
        let package_ref = self.package.compiled().package().clone();
        std::str::from_utf8(&origin).map_err(|_| HttpProtocolProcessError {
            failure: failure_view(
                package_ref.clone(),
                direction,
                None,
                HttpProtocolFailureKind::InputNotUtf8,
                "HTTP_BODY_NOT_UTF8",
                "HTTP 协议处理只接受 UTF-8 文本 Body",
                origin.clone(),
            ),
            error: ProxyError::new(
                ErrorCode::ConfigInvalid,
                "HTTP 协议处理只接受 UTF-8 文本 Body [HTTP_BODY_NOT_UTF8]",
            ),
        })?;
        let origin_for_failure = origin.clone();
        let input = HttpProtocolExecutionInput {
            package: self.package.clone(),
            programs: self.programs.read().clone(),
            origin,
            direction,
            first,
            second,
            connection_id: context.connection_id.to_string(),
            listener_id: self.listener_id.to_string(),
        };
        let execution = tokio::task::spawn_blocking(move || execute_protocol_body(input))
            .await
            .map_err(|_| HttpProtocolProcessError {
                failure: failure_view(
                    package_ref.clone(),
                    direction,
                    None,
                    HttpProtocolFailureKind::WorkerFailed,
                    "HTTP_PROTOCOL_WORKER_FAILED",
                    "HTTP 协议处理任务异常终止",
                    origin_for_failure.clone(),
                ),
                error: ProxyError::new(
                    ErrorCode::Internal,
                    "HTTP 协议处理任务异常终止 [HTTP_PROTOCOL_WORKER_FAILED]",
                ),
            })?;
        let execution = execution.map_err(|(error, stage)| {
            runtime_process_error(
                package_ref.clone(),
                direction,
                origin_for_failure.clone(),
                &error,
                stage,
            )
        })?;
        let output = execution.frame;
        std::str::from_utf8(output.written()).map_err(|_| HttpProtocolProcessError {
            failure: failure_view(
                package_ref.clone(),
                direction,
                None,
                HttpProtocolFailureKind::OutputNotUtf8,
                "HTTP_PROTOCOL_OUTPUT_NOT_UTF8",
                "协议包 Encode 返回了非 UTF-8 HTTP Body",
                origin_for_failure,
            ),
            error: ProxyError::new(
                ErrorCode::Internal,
                "协议包 Encode 返回了非 UTF-8 HTTP Body [HTTP_PROTOCOL_OUTPUT_NOT_UTF8]",
            ),
        })?;
        if output.written() != message.body.as_ref() {
            message.replace_body(Bytes::copy_from_slice(output.written()));
        }
        let origin_text = String::from_utf8(output.origin().to_vec())
            .expect("HTTP protocol input passed the UTF-8 gate");
        let written_text = String::from_utf8(output.written().to_vec())
            .expect("HTTP protocol output passed the UTF-8 gate");
        Ok(Some(HttpProtocolBodyViewModel {
            package: package_ref,
            origin_body: output.origin().to_vec(),
            origin_text,
            written_body: output.written().to_vec(),
            written_text,
            document: output.execution_document().clone(),
            stages: execution.stages,
            display: display_view(execution.display),
        }))
    }
}

fn execute_protocol_body(
    input: HttpProtocolExecutionInput,
) -> Result<HttpProtocolExecutionOutput, HttpProtocolExecutionError> {
    let compiled = input.package.compiled();
    let mut executor = ProtocolDirectionExecutor::new(
        compiled,
        DirectionExecutionPlan::new(input.direction),
        input.connection_id,
        input.listener_id,
        input.package.runtime_limits(),
    )
    .map_err(|error| (error, None))?;
    let first = input.programs.program(input.first);
    let second = input.programs.program(input.second);
    let mut stage_results = Vec::with_capacity(2);
    let mut failed_stage = None;
    let frame = executor
        .execute_message_with_document_transform(input.origin, |document| {
            failed_stage = Some(first.stage());
            let first_execution = first.execute(document).map_err(|_| {
                ProtocolRuntimeError::DocumentTransformFailed {
                    package: compiled.package().clone(),
                }
            })?;
            let (document, matched_rule_ids) = first_execution.into_parts();
            stage_results.push((first.stage(), matched_rule_ids, document.clone()));
            failed_stage = Some(second.stage());
            let second_execution = second.execute(document).map_err(|_| {
                ProtocolRuntimeError::DocumentTransformFailed {
                    package: compiled.package().clone(),
                }
            })?;
            let (document, matched_rule_ids) = second_execution.into_parts();
            stage_results.push((second.stage(), matched_rule_ids, document.clone()));
            failed_stage = None;
            Ok(document)
        })
        .map_err(|error| (error, failed_stage))?;
    let stages = stage_results
        .into_iter()
        .map(
            |(stage, matched_rule_ids, document)| HttpProtocolRuleStageViewModel {
                stage,
                matched_rule_ids,
                display: display_view(executor.render_document_display(&document)),
                document,
            },
        )
        .collect();
    let display = executor.render_output_document_display(&frame);
    Ok(HttpProtocolExecutionOutput {
        frame,
        stages,
        display,
    })
}

impl fmt::Debug for HttpProtocolPipeline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpProtocolPipeline")
            .field("listener_id", &self.snapshot.listener_id)
            .field("package", self.snapshot.package.compiled().package())
            .finish_non_exhaustive()
    }
}

impl HandshakePolicy for HttpProtocolPipeline {
    fn reject_tls_handshake(
        &self,
        context: &ConnectionContext,
        peer: &TlsPeerIdentity,
    ) -> ProxyResult<bool> {
        self.inner.reject_tls_handshake(context, peer)
    }
}

#[async_trait]
impl PipelinePorts for HttpProtocolPipeline {
    async fn runtime_stopping(&self, epoch: Uuid) {
        self.inner.runtime_stopping(epoch).await;
    }

    async fn connection_opened(&self, context: &ConnectionContext) {
        self.inner.connection_opened(context).await;
    }

    async fn upstream_security_established(
        &self,
        context: &ConnectionContext,
        evidence: &UpstreamSecurityEvidence,
    ) {
        self.inner
            .upstream_security_established(context, evidence)
            .await;
    }

    async fn request(
        &self,
        context: &ConnectionContext,
        message: &mut Message,
    ) -> ProxyResult<Vec<FaultAction>> {
        let actions = self.inner.request(context, message).await?;
        let observation = match self
            .snapshot
            .process(
                context,
                message,
                ProtocolDirection::Upstream,
                ProtocolRuleStage::AppToProxy,
                ProtocolRuleStage::ProxyToUpstream,
            )
            .await
        {
            Ok(observation) => observation,
            Err(failure) => {
                self.observations.record_http_protocol_failure(
                    context,
                    message,
                    failure.failure,
                )?;
                return Err(failure.error);
            }
        };
        if let Some(observation) = observation {
            self.observations.record_http_protocol_observation(
                context,
                ProtocolDirection::Upstream,
                message,
                observation,
            )?;
        }
        Ok(actions)
    }

    async fn response(
        &self,
        context: &ConnectionContext,
        message: &mut Message,
    ) -> ProxyResult<Vec<FaultAction>> {
        let actions = self.inner.response(context, message).await?;
        let observation = match self
            .snapshot
            .process(
                context,
                message,
                ProtocolDirection::Downstream,
                ProtocolRuleStage::UpstreamToProxy,
                ProtocolRuleStage::ProxyToApp,
            )
            .await
        {
            Ok(observation) => observation,
            Err(failure) => {
                self.observations.record_http_protocol_failure(
                    context,
                    message,
                    failure.failure,
                )?;
                return Err(failure.error);
            }
        };
        if let Some(observation) = observation {
            self.observations.record_http_protocol_observation(
                context,
                ProtocolDirection::Downstream,
                message,
                observation,
            )?;
        }
        Ok(actions)
    }

    async fn connection_closed(&self, context: &ConnectionContext, result: &ProxyResult<()>) {
        self.inner.connection_closed(context, result).await;
    }

    async fn runtime_fault(&self, epoch: Uuid, channel: ChannelId, error: &ProxyError) {
        self.inner.runtime_fault(epoch, channel, error).await;
    }
}

fn display_view(result: ProtocolDisplayResult) -> HttpProtocolDisplayViewModel {
    match result {
        ProtocolDisplayResult::UntrustedHtml(html) => {
            HttpProtocolDisplayViewModel::UntrustedHtml { html }
        }
        ProtocolDisplayResult::HexFallback(reason) => HttpProtocolDisplayViewModel::HexFallback {
            reason: match reason {
                DisplayFallbackReason::EntryPointFailed => {
                    HttpProtocolDisplayFallbackReason::EntryPointFailed
                }
                DisplayFallbackReason::ResourceLimitExceeded(_) => {
                    HttpProtocolDisplayFallbackReason::ResourceLimitExceeded
                }
            },
        },
    }
}
