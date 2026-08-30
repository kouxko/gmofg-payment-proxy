use async_trait::async_trait;

use super::{
    AppMessageStage, BodyCodec, BreakpointOutcome, BreakpointSummaryViewModel, CapturePublication,
    ConnectionContext, ErrorCode, EvaluatedRules, FaultAction, HandshakePolicy, HttpAction,
    Message, ProxyError, ProxyResult, RuntimePipelineAdapter, TerminalAction, TlsPeerIdentity,
    UiEventPayload, UiTone, Utc, Uuid, app_to_proxy, apply_breakpoint_decision, breakpoint_detail,
};

impl RuntimePipelineAdapter {
    pub(super) async fn pause(
        &self,
        context: &ConnectionContext,
        stage: AppMessageStage,
        original: &Message,
        effective: &mut Message,
        rules: &EvaluatedRules,
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<Vec<FaultAction>> {
        let detail = breakpoint_detail(
            body_codec,
            context,
            self.channel_label(context.channel.as_str()),
            stage,
            original,
            effective,
            self.session_id(context)?,
        )?;
        let breakpoint_id = detail.summary.breakpoint_id;
        let effective_view = detail.effective.clone();
        let ticket = self.breakpoints.register(detail).map_err(app_to_proxy)?;
        {
            let mut pipeline_state = self.state.lock();
            if let Some(connection) = pipeline_state.connection_mut(context) {
                connection.pending_breakpoints.push(breakpoint_id);
            }
        }
        let record = match stage {
            AppMessageStage::Request => self.update_request(
                context,
                effective,
                rules,
                true,
                Some(effective_view),
                body_codec,
            )?,
            AppMessageStage::Response => self.update_response(
                context,
                effective,
                rules,
                true,
                Some(effective_view),
                body_codec,
            )?,
            AppMessageStage::TlsHandshake | AppMessageStage::Terminal => {
                return Err(ProxyError::new(
                    ErrorCode::Internal,
                    "terminal messages cannot enter a breakpoint",
                ));
            }
        };
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            Some(breakpoint_id.to_string()),
            Some(ticket.detail.summary.revision),
            UiEventPayload::BreakpointQueued(ticket.detail.summary.clone()),
        );
        self.publish_capture(
            context,
            &record,
            CapturePublication {
                stage,
                result: "断点等待",
                tone: UiTone::Warning,
                breakpoint_id: Some(breakpoint_id),
                size_bytes: effective.body.len() as u64,
            },
        );

        let outcome = ticket
            .outcome
            .await
            .unwrap_or(BreakpointOutcome::ClientDisconnected);
        self.remove_pending_breakpoint(context, breakpoint_id);
        match outcome {
            BreakpointOutcome::Decision(decision) => {
                let actions = apply_breakpoint_decision(
                    body_codec,
                    stage,
                    original,
                    effective,
                    decision.as_ref(),
                )?;
                match stage {
                    AppMessageStage::Request => {
                        self.update_request(context, effective, rules, false, None, body_codec)?;
                    }
                    AppMessageStage::Response => {
                        self.update_response(context, effective, rules, false, None, body_codec)?;
                    }
                    AppMessageStage::TlsHandshake | AppMessageStage::Terminal => {}
                }
                Ok(actions)
            }
            BreakpointOutcome::ClientDisconnected => Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "客户端已断开，断点已终止。",
            )),
            BreakpointOutcome::ProxyStopped => Err(ProxyError::new(
                ErrorCode::ProxyStopped,
                "Proxy 已停止，断点已终止。",
            )),
        }
    }

    pub(super) fn remove_pending_breakpoint(
        &self,
        context: &ConnectionContext,
        breakpoint_id: Uuid,
    ) {
        if let Some(connection) = self.state.lock().connection_mut(context) {
            connection
                .pending_breakpoints
                .retain(|id| *id != breakpoint_id);
        }
    }

    pub(super) fn session_id(&self, context: &ConnectionContext) -> ProxyResult<Uuid> {
        self.state
            .lock()
            .connection(context)
            .and_then(|connection| connection.session_id)
            .ok_or_else(|| ProxyError::new(ErrorCode::Internal, "connection has no active session"))
    }

    pub(super) fn terminate_connection_breakpoints(
        &self,
        context: &ConnectionContext,
    ) -> Vec<BreakpointSummaryViewModel> {
        let ids = self
            .state
            .lock()
            .connection(context)
            .map_or_else(Vec::new, |connection| {
                connection.pending_breakpoints.clone()
            });
        ids.into_iter()
            .filter_map(|id| self.breakpoints.client_disconnected(id).ok())
            .collect()
    }
}

#[async_trait]
impl HandshakePolicy for RuntimePipelineAdapter {
    async fn prepare_tls_handshake(&self, context: &ConnectionContext) -> ProxyResult<()> {
        self.rule_runtime.prepare_epoch(context.runtime_epoch)
    }

    fn reject_tls_handshake(
        &self,
        context: &ConnectionContext,
        peer: &TlsPeerIdentity,
    ) -> ProxyResult<bool> {
        // rustls invokes this policy while the peer identity is still being
        // verified, before the accepted ConnectionContext can be enriched.
        // Evaluate against a temporary context containing the verified peer.
        let mut verified_context = context.clone();
        verified_context.tls_peer = Some(peer.clone());
        let evaluated = self.rule_runtime.evaluate_handshake(&verified_context)?;
        self.rule_runtime
            .publish_rule_hits(verified_context.runtime_epoch, evaluated.hit_rules);
        Ok(evaluated.actions.into_iter().any(|action| {
            matches!(
                action,
                HttpAction::Terminal(TerminalAction::RejectTlsHandshake)
            )
        }))
    }
}
