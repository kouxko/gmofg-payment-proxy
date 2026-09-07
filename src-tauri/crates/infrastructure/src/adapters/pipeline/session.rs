use super::{
    BodyCodec, ConnectionContext, ErrorCode, EvaluatedRules, LiveSession, Message, ProxyError,
    ProxyResult, RuntimeEpoch, RuntimePipelineAdapter, SessionRecord, SessionStore, UiEventPayload,
    Utc, Uuid, app_channel, app_to_proxy, classify_request, content_view, fingerprint,
    header_value, message_method, message_target, tls_summary,
};
use intercept_proxy_traffic_observation::{
    RuleOutcomeFacts, SessionStartFacts, project_dropped_response, project_request,
    project_response, project_started_session,
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
        let record = project_started_session(
            SessionStartFacts {
                session_id,
                request_id,
                started_at: now,
                terminal_ip,
                channel: app_channel(&context.channel)?,
                channel_text: self.channel_label(context.channel.as_str()),
                method,
                target,
                runtime_epoch: context.runtime_epoch,
                connection_id: context.connection_id.to_string(),
                certificate_fingerprint: fingerprint,
                upstream_host: header_value(original, "host").unwrap_or_default(),
                app_to_proxy_tls: tls_summary(context),
                request_size_bytes: original.body.len() as u64,
            },
            request,
        );
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
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<SessionRecord> {
        let request = content_view(body_codec, effective);
        self.update_live_session(context, move |record| {
            project_request(
                record,
                request,
                effective.body.len() as u64,
                RuleOutcomeFacts {
                    matched_rule_ids: &rules.matched_ids,
                    traces: &rules.traces,
                },
            );
        })
    }

    pub(super) fn update_response(
        &self,
        context: &ConnectionContext,
        effective: &Message,
        rules: &EvaluatedRules,
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<SessionRecord> {
        let response = content_view(body_codec, effective);
        self.update_live_session(context, move |record| {
            project_response(
                record,
                response,
                effective.body.len() as u64,
                RuleOutcomeFacts {
                    matched_rule_ids: &rules.matched_ids,
                    traces: &rules.traces,
                },
            );
        })
    }

    pub(super) fn update_dropped_response(
        &self,
        context: &ConnectionContext,
        rules: &EvaluatedRules,
    ) -> ProxyResult<SessionRecord> {
        self.update_live_session(context, move |record| {
            project_dropped_response(
                record,
                RuleOutcomeFacts {
                    matched_rule_ids: &rules.matched_ids,
                    traces: &rules.traces,
                },
            );
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
