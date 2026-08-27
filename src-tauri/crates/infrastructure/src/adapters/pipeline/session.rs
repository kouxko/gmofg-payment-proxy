use super::{
    BTreeMap, BodyCodec, ConnectionContext, ErrorCode, EvaluatedRules, LiveSession, Message,
    MessageContentViewModel, ProxyError, ProxyResult, RuntimeEpoch, RuntimePipelineAdapter,
    SessionDetailViewModel, SessionRecord, SessionStore, SessionSummaryViewModel, UiEventPayload,
    UiTone, Utc, Uuid, app_channel, app_to_proxy, classify_request, content_view, fingerprint,
    header_value, message_method, message_target, tls_summary,
};

impl RuntimePipelineAdapter {
    pub(super) fn begin_session(
        &self,
        context: &ConnectionContext,
        original: &Message,
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<Uuid> {
        {
            let state = self.state.lock();
            let epoch = RuntimeEpoch::from_uuid(context.runtime_epoch);
            if !state.active_epochs.contains(&epoch) {
                return Err(ProxyError::new(
                    ErrorCode::ProxyStopped,
                    "runtime epoch is already stopping",
                ));
            }
            if state.connection(context).is_none() {
                return Err(ProxyError::new(
                    ErrorCode::Internal,
                    "connection was not registered before request processing",
                ));
            }
        }
        let now = Utc::now();
        let session_id = Uuid::new_v4();
        let request = content_view(body_codec, original);
        let classified =
            classify_request(self.request_classifier.as_ref(), &context.channel, original);
        let request_id = classified
            .request_id
            .unwrap_or_else(|| session_id.to_string());
        let terminal_ip = context.peer_addr.ip().to_string();
        let fingerprint = fingerprint(context);
        let target = classified.request_type.unwrap_or_else(|| {
            message_target(&original.start_line)
                .unwrap_or_default()
                .to_owned()
        });
        let method = message_method(&original.start_line)
            .unwrap_or_default()
            .to_owned();
        let summary = SessionSummaryViewModel {
            session_id,
            request_id: request_id.clone(),
            started_at: now,
            completed_at: None,
            terminal_ip: terminal_ip.clone(),
            channel: app_channel(&context.channel)?,
            channel_text: self.channel_label(context.channel.as_str()),
            method,
            target,
            http_status: None,
            result: "处理中".into(),
            ui_tone: UiTone::Info,
            duration_ms: None,
            matched_rule_ids: Vec::new(),
            request_size_bytes: original.body.len() as u64,
            response_size_bytes: 0,
            pending_breakpoint: false,
            revision: 1,
        };
        let detail = SessionDetailViewModel {
            summary,
            runtime_epoch: context.runtime_epoch,
            connection_id: context.connection_id.to_string(),
            certificate_fingerprint: fingerprint.clone(),
            upstream_host: header_value(original, "host").unwrap_or_default(),
            app_to_proxy_tls: tls_summary(context),
            proxy_to_server_tls: "上游安全信息等待传输层上报".into(),
            final_action: "处理中".into(),
            timings_ms: BTreeMap::new(),
            request: Some(request),
            response: None,
            rule_trace: Vec::new(),
        };
        let record = SessionRecord {
            detail,
            breakpoint_draft: None,
        };
        if let Err(error) = self.sessions.upsert(record.clone()) {
            self.resource_exhausted(context, &error);
            return Err(app_to_proxy(error));
        }
        {
            let mut state = self.state.lock();
            state.live_sessions.insert(
                session_id,
                LiveSession {
                    started_at: now,
                    runtime_epoch: context.runtime_epoch,
                },
            );
            if let Some(connection) = state.connection_mut(context) {
                connection.session_id = Some(session_id);
            }
            if let Some(metrics) = state.channel_metrics_mut(context) {
                metrics.request_count = metrics.request_count.saturating_add(1);
            }
        }
        Ok(session_id)
    }

    pub(super) fn update_request(
        &self,
        context: &ConnectionContext,
        effective: &Message,
        rules: &EvaluatedRules,
        pending_breakpoint: bool,
        breakpoint_draft: Option<MessageContentViewModel>,
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<SessionRecord> {
        self.update_live_session(context, move |record| {
            let summary = &mut record.detail.summary;
            summary.matched_rule_ids.clone_from(&rules.matched_ids);
            summary.request_size_bytes = effective.body.len() as u64;
            summary.pending_breakpoint = pending_breakpoint;
            summary.result = if pending_breakpoint {
                "断点等待".into()
            } else {
                "请求已处理".into()
            };
            summary.ui_tone = if pending_breakpoint {
                UiTone::Warning
            } else {
                UiTone::Info
            };
            summary.revision = summary.revision.saturating_add(1);
            record.detail.request = Some(content_view(body_codec, effective));
            record.detail.rule_trace.clone_from(&rules.traces);
            record.breakpoint_draft = breakpoint_draft;
        })
    }

    pub(super) fn update_response(
        &self,
        context: &ConnectionContext,
        effective: &Message,
        rules: &EvaluatedRules,
        pending_breakpoint: bool,
        breakpoint_draft: Option<MessageContentViewModel>,
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<SessionRecord> {
        self.update_live_session(context, move |record| {
            let summary = &mut record.detail.summary;
            for id in &rules.matched_ids {
                if !summary.matched_rule_ids.contains(id) {
                    summary.matched_rule_ids.push(*id);
                }
            }
            summary.response_size_bytes = effective.body.len() as u64;
            summary.http_status = effective.http_status();
            summary.pending_breakpoint = pending_breakpoint;
            summary.result = if pending_breakpoint {
                "断点等待".into()
            } else {
                "响应已处理".into()
            };
            summary.ui_tone = if pending_breakpoint {
                UiTone::Warning
            } else {
                UiTone::Info
            };
            summary.revision = summary.revision.saturating_add(1);
            record.detail.response = Some(content_view(body_codec, effective));
            record.detail.rule_trace.extend(rules.traces.clone());
            record.breakpoint_draft = breakpoint_draft;
        })
    }

    pub(super) fn update_dropped_response(
        &self,
        context: &ConnectionContext,
        rules: &EvaluatedRules,
    ) -> ProxyResult<SessionRecord> {
        self.update_live_session(context, move |record| {
            let summary = &mut record.detail.summary;
            for id in &rules.matched_ids {
                if !summary.matched_rule_ids.contains(id) {
                    summary.matched_rule_ids.push(*id);
                }
            }
            summary.response_size_bytes = 0;
            summary.http_status = None;
            summary.pending_breakpoint = false;
            summary.result = "响应已丢弃".into();
            summary.ui_tone = UiTone::Danger;
            summary.revision = summary.revision.saturating_add(1);
            record.detail.response = None;
            record.detail.rule_trace.extend(rules.traces.clone());
            record.breakpoint_draft = None;
        })
    }

    pub(super) fn update_live_session(
        &self,
        context: &ConnectionContext,
        update: impl FnOnce(&mut SessionRecord),
    ) -> ProxyResult<SessionRecord> {
        let session_id = {
            let state = self.state.lock();
            let session_id = state
                .connection(context)
                .and_then(|connection| connection.session_id)
                .ok_or_else(|| {
                    ProxyError::new(ErrorCode::Internal, "connection has no active session")
                })?;
            if !state.live_sessions.contains_key(&session_id) {
                return Err(ProxyError::new(
                    ErrorCode::Internal,
                    "active session metadata is missing",
                ));
            }
            session_id
        };
        let mut record = self.sessions.get_record(session_id).map_err(app_to_proxy)?;
        update(&mut record);
        if let Err(error) = self.sessions.upsert(record.clone()) {
            self.resource_exhausted(context, &error);
            return Err(app_to_proxy(error));
        }
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            Some(record.id().to_string()),
            Some(record.detail.summary.revision),
            UiEventPayload::SessionUpdated(record.detail.summary.clone()),
        );
        Ok(record)
    }
}
