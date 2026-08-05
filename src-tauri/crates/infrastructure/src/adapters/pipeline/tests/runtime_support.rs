#[test]
fn codec_failures_keep_generic_stable_error_codes() {
    let codec = Utf8BodyCodec;
    let decode_error = decode_body(&codec, &[0xff]).expect_err("invalid UTF-8");
    assert_eq!(decode_error.code, "BODY_DECODE_FAILED");

    let encode_error = encode_body(&RejectingCodec, "body").expect_err("rejected body");
    assert_eq!(encode_error.code, "PRODUCT_SPECIFIC_CODE");
}

#[derive(Debug)]
struct StaticRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
}

#[derive(Debug)]
struct RejectingCommitRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
    reject_commit: AtomicBool,
}

#[derive(Debug)]
struct ConflictOnceRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
    conflict_once: AtomicBool,
    commit_attempts: AtomicUsize,
}

impl RuntimeRuleRepository for RejectingCommitRules {
    fn runtime_snapshot(&self, _channel: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    fn commit_runtime_snapshot(&self, _: &RuleRuntimeSnapshot, _: &[Rule]) -> AppResult<u64> {
        if self.reject_commit.load(AtomicOrdering::Acquire) {
            Err(AppError::new(
                "REVISION_CONFLICT",
                "模拟运行态事务提交失败。",
            ))
        } else {
            Ok(1)
        }
    }

    fn reset_runtime_hit_metadata(&self, _collection_id: Uuid) -> AppResult<()> {
        Ok(())
    }
}

impl RuntimeRuleRepository for StaticRules {
    fn runtime_snapshot(&self, _channel: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    fn commit_runtime_snapshot(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        evaluated_rules: &[Rule],
    ) -> AppResult<u64> {
        let mut current = self.snapshot.lock();
        if current.collection_id != snapshot.collection_id
            || current.signature != snapshot.signature
            || current.collection_revision != snapshot.collection_revision
        {
            return Err(AppError::new("REVISION_CONFLICT", "规则测试快照已变化。"));
        }
        let next_revision = current.collection_revision.saturating_add(1);
        *current = RuleRuntimeSnapshot::with_collection_identity(
            snapshot.collection_id,
            next_revision,
            evaluated_rules.to_vec(),
        );
        Ok(next_revision)
    }

    fn reset_runtime_hit_metadata(&self, _collection_id: Uuid) -> AppResult<()> {
        let mut current = self.snapshot.lock();
        for rule in &mut current.rules {
            rule.hit_count = 0;
            rule.last_hit_at = None;
        }
        let next_revision = current.collection_revision.saturating_add(1);
        *current = RuleRuntimeSnapshot::with_collection_identity(
            current.collection_id,
            next_revision,
            current.rules.clone(),
        );
        Ok(())
    }
}

impl RuntimeRuleRepository for ConflictOnceRules {
    fn runtime_snapshot(&self, _channel: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    fn commit_runtime_snapshot(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        evaluated_rules: &[Rule],
    ) -> AppResult<u64> {
        self.commit_attempts.fetch_add(1, AtomicOrdering::AcqRel);
        let mut current = self.snapshot.lock();
        if self.conflict_once.swap(false, AtomicOrdering::AcqRel) {
            let externally_advanced_revision = current.collection_revision.saturating_add(1);
            let externally_preserved_rules = current.rules.clone();
            *current = RuleRuntimeSnapshot::with_collection_identity(
                current.collection_id,
                externally_advanced_revision,
                externally_preserved_rules,
            );
            return Err(AppError::new(
                "REVISION_CONFLICT",
                "模拟评估后发生外部规则集合更新。",
            ));
        }
        if current.collection_id != snapshot.collection_id
            || current.signature != snapshot.signature
            || current.collection_revision != snapshot.collection_revision
        {
            return Err(AppError::new("REVISION_CONFLICT", "规则测试快照已变化。"));
        }
        let next_revision = current.collection_revision.saturating_add(1);
        *current = RuleRuntimeSnapshot::with_collection_identity(
            snapshot.collection_id,
            next_revision,
            evaluated_rules.to_vec(),
        );
        Ok(next_revision)
    }

    fn reset_runtime_hit_metadata(&self, _collection_id: Uuid) -> AppResult<()> {
        Ok(())
    }
}

fn pause_rule() -> RuleViewModel {
    let id = Uuid::new_v4();
    RuleViewModel {
        summary: RuleSummaryViewModel {
            rule_id: id,
            revision: 1,
            name: "暂停请求".into(),
            enabled: true,
            priority: 1,
            creation_order: 1,
            channel_text: "全部".into(),
            stage_text: "请求".into(),
            match_summary: "0 个条件".into(),
            action_summary: "1 个动作".into(),
            hit_count: 0,
            last_hit_at: None,
            ui_tone: UiTone::Positive,
        },
        draft: AppRuleDraft {
            rule_id: Some(id),
            expected_revision: Some(1),
            name: "暂停请求".into(),
            description: String::new(),
            enabled: true,
            priority: 1,
            channel: None,
            stage: Some(AppMessageStage::Request),
            conditions: Vec::new(),
            actions: vec![intercept_proxy_application::RuleAction::Pause],
            one_shot: false,
        },
    }
}

fn one_shot_delay_rule() -> RuleViewModel {
    let mut rule = pause_rule();
    rule.summary.name = "一次性延迟".into();
    rule.draft.name = "一次性延迟".into();
    rule.draft.actions = vec![intercept_proxy_application::RuleAction::Delay { milliseconds: 25 }];
    rule.draft.one_shot = true;
    rule
}

fn response_status_rule(status: u16) -> RuleViewModel {
    let mut rule = pause_rule();
    rule.summary.name = "响应状态替换".into();
    rule.summary.stage_text = "响应".into();
    rule.draft.name = "响应状态替换".into();
    rule.draft.stage = Some(AppMessageStage::Response);
    rule.draft.actions = vec![intercept_proxy_application::RuleAction::CustomHttpStatus { status }];
    rule
}

fn tls_fingerprint_reject_rule(fingerprint: &str) -> RuleViewModel {
    let id = Uuid::new_v4();
    RuleViewModel {
        summary: RuleSummaryViewModel {
            rule_id: id,
            revision: 1,
            name: "拒绝指定证书".into(),
            enabled: true,
            priority: 1,
            creation_order: 1,
            channel_text: "全部".into(),
            stage_text: "TLS 握手".into(),
            match_summary: "证书指纹".into(),
            action_summary: "拒绝 TLS".into(),
            hit_count: 0,
            last_hit_at: None,
            ui_tone: UiTone::Positive,
        },
        draft: AppRuleDraft {
            rule_id: Some(id),
            expected_revision: Some(1),
            name: "拒绝指定证书".into(),
            description: String::new(),
            enabled: true,
            priority: 1,
            channel: None,
            stage: Some(AppMessageStage::TlsHandshake),
            conditions: vec![intercept_proxy_application::RuleCondition::Field {
                field: intercept_proxy_application::RuleMatchField::CertificateFingerprint,
                operator: intercept_proxy_application::RuleMatchOperator::Equals {
                    value: fingerprint.into(),
                },
            }],
            actions: vec![intercept_proxy_application::RuleAction::Terminal {
                action: intercept_proxy_application::RuleTerminalAction::RejectTlsHandshake,
            }],
            one_shot: false,
        },
    }
}

fn adapter(views: Vec<RuleViewModel>, max_sessions: usize) -> Arc<RuntimePipelineAdapter> {
    let rules = views
        .into_iter()
        .map(view_to_domain_rule)
        .collect::<ProxyResult<Vec<_>>>()
        .expect("valid test rules");
    Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(),
        Arc::new(StaticRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(rules)),
        }),
        Arc::new(InMemorySessionStore::new(max_sessions, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(128)),
        Arc::new(CaptureRepositoryAdapter::default()),
    ))
}

fn transaction_channel() -> ChannelId {
    ChannelId::new("transaction").expect("valid transaction channel")
}

fn dll_channel() -> ChannelId {
    ChannelId::new("dll").expect("valid DLL channel")
}

fn test_context(epoch: Uuid, connection_id: Uuid, channel: ChannelId) -> ConnectionContext {
    ConnectionContext {
        runtime_epoch: epoch,
        connection_id,
        channel,
        peer_addr: "10.0.0.2:12345".parse::<SocketAddr>().expect("address"),
        accepted_at: SystemTime::now(),
        tls_peer: Some(TlsPeerIdentity {
            sha256_fingerprint: "AA:BB:CC:DD:EE:FF".into(),
            subject_summary: "CN=Test Client".into(),
        }),
    }
}

fn upstream_tls_evidence(peer_subject: impl Into<String>) -> UpstreamSecurityEvidence {
    UpstreamSecurityEvidence {
        resolved_address: "127.0.0.1:16627".parse().unwrap(),
        transport: intercept_proxy_runtime::UpstreamTransportSecurity::Tls,
        tls_version: Some("TLS 1.2".into()),
        cipher_suite: Some("TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256".into()),
        peer_subject: Some(peer_subject.into()),
        peer_sha256_fingerprint: Some("AA:BB:CC".into()),
        hostname_verification_enabled: Some(true),
        client_identity_configured: true,
        client_identity_submitted: true,
    }
}

fn request_message(body: &str) -> Message {
    Message {
        start_line: "POST /payment HTTP/1.1".into(),
        headers: vec![
            RawHeader::new(b"host".to_vec(), b"example.test".to_vec()),
            RawHeader::new(b"x-request-id".to_vec(), b"REQ-1".to_vec()),
        ],
        body: body.as_bytes().to_vec().into(),
        body_modified: false,
    }
}

fn response_message() -> Message {
    Message {
        start_line: "HTTP/1.1 200 OK".into(),
        headers: vec![RawHeader::new(b"x-server".to_vec(), b"gmo-fg".to_vec())],
        body: br#"{"result":"ok"}"#.to_vec().into(),
        body_modified: false,
    }
}
