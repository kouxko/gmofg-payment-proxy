mod runtime_lifecycle;
use runtime_lifecycle::reset_rule_lifecycle;

#[test]
fn codec_failures_keep_generic_stable_error_codes() {
    let codec = Utf8BodyCodec;
    let decode_error = decode_body(&codec, &[0xff]).expect_err("invalid UTF-8");
    assert_eq!(decode_error.code, "BODY_DECODE_FAILED");

    let encode_error = encode_body(&RejectingCodec, "body").expect_err("rejected body");
    assert_eq!(encode_error.code, "PRODUCT_SPECIFIC_CODE");
}

#[derive(Debug)]
struct RejectingCodec;

impl BodyCodec for RejectingCodec {
    fn id(&self) -> &'static str {
        "rejecting"
    }

    fn name(&self) -> &'static str {
        "Rejecting Codec"
    }

    fn decode(&self, _: &[u8]) -> Result<String, intercept_proxy_product_api::ProductError> {
        Ok(String::new())
    }

    fn encode(&self, _: &str) -> Result<Vec<u8>, intercept_proxy_product_api::ProductError> {
        Err(intercept_proxy_product_api::ProductError::new(
            "PRODUCT_SPECIFIC_CODE",
            "rejected",
        ))
    }
}

#[derive(Debug)]
struct StaticRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
}

#[async_trait]
impl RuntimeRuleRepository for StaticRules {
    async fn runtime_snapshot(&self, _channel: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    async fn commit_runtime_deltas(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        deltas: &[intercept_proxy_domain::RuleLifecycleDelta],
    ) -> AppResult<u64> {
        let mut current = self.snapshot.lock();
        if current.collection_id != snapshot.collection_id
            || current.signature != snapshot.signature
            || current.collection_revision != snapshot.collection_revision
        {
            return Err(AppError::new("REVISION_CONFLICT", "规则测试快照已变化。"));
        }
        let next_revision = current.collection_revision.saturating_add(1);
        *current = RuleRuntimeSnapshot::with_collection_identity_and_order(
            snapshot.collection_id,
            next_revision,
            crate::adapters::rules::conversion::apply_runtime_deltas(snapshot, deltas)?,
            snapshot.execution_order.clone(),
        );
        Ok(next_revision)
    }

    async fn reset_runtime_hit_metadata(&self, _collection_id: Uuid) -> AppResult<()> {
        let mut current = self.snapshot.lock();
        current.rules = current
            .rules
            .iter()
            .map(reset_rule_lifecycle)
            .collect::<AppResult<Vec<_>>>()?;
        let next_revision = current.collection_revision.saturating_add(1);
        *current = RuleRuntimeSnapshot::with_collection_identity_and_order(
            current.collection_id,
            next_revision,
            current.rules.clone(),
            current.execution_order.clone(),
        );
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_rule_repository_contract_is_async() {
    let repository = StaticRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(Vec::new())),
    };

    let snapshot = RuntimeRuleRepository::runtime_snapshot(&repository, &transaction_channel())
        .await
        .unwrap();

    assert!(snapshot.rules.is_empty());
}

fn adapter(rules: Vec<RuleDefinition>, max_sessions: usize) -> Arc<RuntimePipelineAdapter> {
    Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(),
        Arc::new(StaticRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(rules)),
        }),
        Arc::new(InMemorySessionStore::new(max_sessions, 64 * 1024 * 1024)),
        Arc::new(EventHub::new(128)),
        test_capture_repository(),
    ))
}

fn transaction_channel() -> ChannelId {
    ChannelId::new(Uuid::from_u128(0x7472).to_string()).expect("valid transaction channel")
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

async fn open_test_connection(
    pipeline: &RuntimePipelineAdapter,
    context: &ConnectionContext,
) {
    pipeline.runtime_started(context.runtime_epoch).await;
    pipeline.connection_opened(context).await;
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
