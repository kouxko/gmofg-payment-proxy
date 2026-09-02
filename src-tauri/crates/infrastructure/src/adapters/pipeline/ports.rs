use async_trait::async_trait;

use super::{
    AppError, AppMessageStage, CapturePublication, ChannelId, ConnectionContext, ConnectionRuntime,
    DomainMessageStage, FaultAction, HttpRequestMetadata, Message, PipelinePorts, ProxyError,
    ProxyResult, RuntimeEpoch, RuntimePipelineAdapter, SessionStore, UiEventPayload, UiTone,
    UpstreamSecurityEvidence, Utc, Uuid, mock_response, project_response_for_observation,
    upstream_security,
};

#[async_trait]
impl PipelinePorts for RuntimePipelineAdapter {
    async fn runtime_started(&self, epoch: Uuid) {
        if self.rule_runtime.runtime_started(epoch).is_ok() {
            self.state
                .lock()
                .active_epochs
                .insert(RuntimeEpoch::from_uuid(epoch));
        }
    }

    async fn runtime_stopping(&self, epoch: Uuid) {
        self.rule_runtime.runtime_stopping(epoch).await;
        let epoch = RuntimeEpoch::from_uuid(epoch);
        let mut state = self.state.lock();
        state.channels.remove(&epoch);
        state.active_epochs.remove(&epoch);
    }

    async fn connection_opened(&self, context: &ConnectionContext) {
        let mut state = self.state.lock();
        let epoch = RuntimeEpoch::from_uuid(context.runtime_epoch);
        if !state.active_epochs.contains(&epoch) {
            return;
        }
        state.connections.entry(epoch).or_default().insert(
            context.connection_id,
            ConnectionRuntime {
                channel: context.channel.clone(),
                session_id: None,
            },
        );
        let metrics = state
            .channels
            .entry(epoch)
            .or_default()
            .entry(context.channel.clone())
            .or_default();
        metrics.connected_clients = metrics.connected_clients.saturating_add(1);
    }

    async fn upstream_security_established(
        &self,
        context: &ConnectionContext,
        evidence: &UpstreamSecurityEvidence,
    ) {
        let evidence_text = upstream_security::describe(evidence);
        let session_id = {
            let state = self.state.lock();
            state
                .connection(context)
                .and_then(|connection| connection.session_id)
        };
        let Some(session_id) = session_id else {
            self.upstream_security_persistence_failed(
                context,
                context.connection_id.to_string(),
                AppError::new(
                    "UPSTREAM_SECURITY_SESSION_MISSING",
                    "当前连接尚未关联可写入的会话记录。",
                )
                .retryable("重新发起请求；若持续发生，请停止并重新启动该代理入口。"),
                &evidence_text,
            );
            return;
        };
        let mut record = match self.sessions.get_record(session_id) {
            Ok(record) => record,
            Err(error) => {
                self.upstream_security_persistence_failed(
                    context,
                    session_id.to_string(),
                    AppError::new(
                        "UPSTREAM_SECURITY_SESSION_MISSING",
                        error.view_model.message,
                    )
                    .retryable("重新查询会话；若持续发生，请停止并重新启动该代理入口。"),
                    &evidence_text,
                );
                return;
            }
        };
        record.detail.proxy_to_server_tls.clone_from(&evidence_text);
        if let Err(error) = self.sessions.upsert(record) {
            self.upstream_security_persistence_failed(
                context,
                session_id.to_string(),
                error,
                &evidence_text,
            );
        }
    }

    async fn apply_request_policy(
        &self,
        context: &ConnectionContext,
        request: &HttpRequestMetadata,
        message: &mut Message,
    ) -> ProxyResult<Vec<FaultAction>> {
        let body_codec = self.codec_for(context, DomainMessageStage::Request, message)?;
        self.begin_session(context, message, body_codec.as_ref())?;
        let evaluated = self
            .evaluate(
                context,
                DomainMessageStage::Request,
                request,
                Some(message),
                body_codec.clone(),
            )
            .await?;
        *message = evaluated
            .prepared_message
            .clone()
            .expect("request evaluation returns a prepared message");
        let actions = evaluated.fault_actions.clone();
        self.rule_runtime
            .publish_rule_hits(context.runtime_epoch, evaluated.hit_rules.clone());
        let record = self.update_request(context, message, &evaluated, body_codec.as_ref())?;
        self.publish_capture(
            context,
            &record,
            CapturePublication {
                stage: AppMessageStage::Request,
                result: "请求",
                tone: UiTone::Info,
                size_bytes: message.body.len() as u64,
            },
        );
        if let Some(FaultAction::MockResponse {
            status,
            headers,
            body,
        }) = actions
            .iter()
            .find(|action| matches!(action, FaultAction::MockResponse { .. }))
        {
            let mock = mock_response(*status, headers, body.clone());
            let response_codec = self.codec_for(context, DomainMessageStage::Response, &mock)?;
            let record =
                self.update_response(context, &mock, &evaluated, response_codec.as_ref())?;
            self.publish_capture(
                context,
                &record,
                CapturePublication {
                    stage: AppMessageStage::Response,
                    result: "Mock 响应",
                    tone: UiTone::Warning,
                    size_bytes: mock.body.len() as u64,
                },
            );
        }
        Ok(actions)
    }

    async fn apply_response_policy(
        &self,
        context: &ConnectionContext,
        request: &HttpRequestMetadata,
        message: &mut Message,
    ) -> ProxyResult<Vec<FaultAction>> {
        let body_codec = self.codec_for(context, DomainMessageStage::Response, message)?;
        {
            let mut state = self.state.lock();
            if let Some(metrics) = state.channel_metrics_mut(context) {
                metrics.upstream_response_count = metrics.upstream_response_count.saturating_add(1);
                metrics.last_upstream_error = None;
            }
        }
        let evaluated = self
            .evaluate(
                context,
                DomainMessageStage::Response,
                request,
                Some(message),
                body_codec.clone(),
            )
            .await?;
        *message = evaluated
            .prepared_message
            .clone()
            .expect("response evaluation returns a prepared message");
        let actions = evaluated.fault_actions.clone();
        self.rule_runtime
            .publish_rule_hits(context.runtime_epoch, evaluated.hit_rules.clone());
        if let Some(observed) = project_response_for_observation(message.clone(), &actions)? {
            let record =
                self.update_response(context, &observed, &evaluated, body_codec.as_ref())?;
            self.publish_capture(
                context,
                &record,
                CapturePublication {
                    stage: AppMessageStage::Response,
                    result: "响应",
                    tone: UiTone::Info,
                    size_bytes: observed.body.len() as u64,
                },
            );
        } else {
            let record = self.update_dropped_response(context, &evaluated)?;
            self.publish_capture(
                context,
                &record,
                CapturePublication {
                    stage: AppMessageStage::Response,
                    result: "响应已丢弃",
                    tone: UiTone::Danger,
                    size_bytes: 0,
                },
            );
        }
        Ok(actions)
    }

    async fn apply_socket_policy(
        &self,
        context: &ConnectionContext,
        direction: intercept_proxy_runtime::SocketPayloadDirection,
        evaluation: Box<dyn intercept_proxy_runtime::SocketJointEvaluation>,
    ) -> ProxyResult<intercept_proxy_exchange::SocketContext> {
        let stage = match direction {
            intercept_proxy_runtime::SocketPayloadDirection::AppToUpstream
            | intercept_proxy_runtime::SocketPayloadDirection::LocalExchange => {
                DomainMessageStage::Request
            }
            intercept_proxy_runtime::SocketPayloadDirection::UpstreamToApp => {
                DomainMessageStage::Response
            }
        };
        let evaluated = self
            .rule_runtime
            .evaluate_socket(context, stage, evaluation)
            .await?;
        self.rule_runtime
            .publish_rule_hits(context.runtime_epoch, evaluated.hit_rules);
        evaluated.prepared_socket.ok_or_else(|| {
            ProxyError::new(
                intercept_proxy_runtime::ErrorCode::Internal,
                "Socket rule transaction did not prepare encoded output",
            )
        })
    }

    async fn connection_closed(&self, context: &ConnectionContext, result: &ProxyResult<()>) {
        {
            // Update health metrics before SessionUpdated is published so a UI
            // refresh triggered by that event observes the new global state.
            let mut state = self.state.lock();
            let channel = state
                .connection(context)
                .map(|connection| connection.channel.clone());
            if let Some(metrics) = channel.and_then(|channel| {
                state
                    .channels
                    .get_mut(&RuntimeEpoch::from_uuid(context.runtime_epoch))?
                    .get_mut(&channel)
            }) {
                metrics.connected_clients = metrics.connected_clients.saturating_sub(1);
                if let Err(error) = result {
                    metrics.error_count = metrics.error_count.saturating_add(1);
                    if is_upstream_error(error.code) {
                        metrics.last_upstream_error = Some(error.message.clone());
                    }
                }
            }
        }
        if let Err(error) = result {
            // TLS 接受失败发生在 connection_opened 之前，因此没有 SessionUpdated 可以
            // 承载错误。如果只更新计数，Android 侧只能看到模糊的 EOF，诊断页也无法
            // 区分 TLS 协议、证书或 HTTP 管线错误。统一发布稳定错误码，同时用
            // channel 作为实体 ID，使诊断日志能准确归属到发生失败的代理入口。
            self.events.publish(
                Some(context.runtime_epoch),
                Utc::now(),
                None,
                None,
                UiEventPayload::OperationFailed(
                    AppError::new(error.code, error.message.clone())
                        .entity(context.channel.as_str())
                        .epoch(context.runtime_epoch)
                        .into(),
                ),
            );
        }
        self.finish_session(context, result);
        self.joint_http_rules.remove_connection(context);
        self.state.lock().remove_connection(context);
    }

    async fn runtime_fault(&self, epoch: Uuid, channel: ChannelId, error: &ProxyError) {
        {
            let mut state = self.state.lock();
            if let Some(metrics) = state
                .channels
                .get_mut(&RuntimeEpoch::from_uuid(epoch))
                .and_then(|channels| channels.get_mut(&channel))
            {
                metrics.error_count = metrics.error_count.saturating_add(1);
            }
        }
        self.events.publish(
            Some(epoch),
            Utc::now(),
            None,
            None,
            UiEventPayload::OperationFailed(
                AppError::new(error.code, error.message.clone()).into(),
            ),
        );
    }
}

#[async_trait]
impl super::HandshakePolicy for RuntimePipelineAdapter {
    async fn prepare_tls_handshake(&self, context: &ConnectionContext) -> ProxyResult<()> {
        self.rule_runtime.prepare_epoch(context.runtime_epoch)
    }
}

fn is_upstream_error(code: &str) -> bool {
    matches!(
        code,
        "UPSTREAM_CONNECT_TIMEOUT"
            | "UPSTREAM_WRITE_TIMEOUT"
            | "UPSTREAM_READ_TIMEOUT"
            | "TLS_HANDSHAKE_FAILED"
            | "IO_ERROR"
    )
}
