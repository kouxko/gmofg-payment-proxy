use std::future::pending;
use std::io;
use std::sync::atomic::AtomicUsize;

use crate::fault::FaultAction;
use crate::http::{ForwardRequest, NoopPipelinePorts, UpstreamConnector};
use crate::transport::{
    AcceptedConnection, BoxIo, ConnectionAcceptor, HandshakePolicy, SystemClock,
    TokioListenerBinder,
};

use super::*;

#[derive(Debug)]
struct PlaintextAcceptor;

#[async_trait]
impl ConnectionAcceptor for PlaintextAcceptor {
    async fn accept(
        &self,
        io: BoxIo,
        _context: &crate::transport::ConnectionContext,
    ) -> Result<AcceptedConnection> {
        Ok(AcceptedConnection { io, tls_peer: None })
    }
}

#[derive(Debug)]
struct UnusedUpstream;

#[async_trait]
impl UpstreamConnector for UnusedUpstream {
    async fn send(
        &self,
        _context: &crate::transport::ConnectionContext,
        _ports: &dyn PipelinePorts,
        _request: ForwardRequest,
        _actions: &[FaultAction],
        _informational: Option<&crate::http::InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> Result<crate::http::UpstreamExchange> {
        unreachable!("the synthetic listeners never accept a connection")
    }
}

fn test_service(ports: Arc<dyn PipelinePorts>) -> ConnectionService {
    ConnectionService {
        acceptor: Arc::new(PlaintextAcceptor),
        upstream: Arc::new(UnusedUpstream),
        ports,
        capabilities: Arc::new(crate::http::PlainHttpCapabilityFactory::new(
            "supervisor-test-workspace",
            "supervisor-test-listener",
        )),
        endpoint: "unused.test:443".into(),
        clock: Arc::new(SystemClock),
        admission: ConnectionAdmission::new(8).unwrap(),
        allowed_client_cidrs: Vec::new(),
        limits: MessageLimits::default(),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
    }
}

fn channel_id(value: &str) -> ChannelId {
    ChannelId::new(value).expect("valid test channel ID")
}

fn test_config() -> ProxyConfig {
    ProxyConfig {
        channels: vec![
            ChannelConfig {
                channel: channel_id("alpha"),
                enabled: true,
                listen_addr: "127.0.0.1:0".parse().unwrap(),
                upstream_url: "http://alpha.test/".into(),
            },
            ChannelConfig {
                channel: channel_id("beta"),
                enabled: true,
                listen_addr: "127.0.0.1:0".parse().unwrap(),
                upstream_url: "http://beta.test/".into(),
            },
        ],
        limits: MessageLimits::default(),
        max_connections: 8,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        rewrite_host: true,
        leaf_sans: vec!["localhost".into()],
    }
}

#[test]
fn channel_id_accepts_safe_product_neutral_values() {
    for value in ["alpha", "beta-v2", "gamma_3", "region.eu"] {
        assert_eq!(channel_id(value).as_str(), value);
    }
    for value in ["", "-alpha", "alpha-", "alpha/beta", "日本語"] {
        let error = ChannelId::new(value).expect_err("unsafe channel ID is rejected");
        assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
    }
}

#[test]
fn channel_id_serde_round_trip_preserves_validation() {
    let original = channel_id("region.eu-2");
    let json = serde_json::to_string(&original).expect("serialize channel ID");
    assert_eq!(json, "\"region.eu-2\"");
    assert_eq!(
        serde_json::from_str::<ChannelId>(&json).expect("deserialize channel ID"),
        original
    );
    assert!(serde_json::from_str::<ChannelId>("\"alpha/beta\"").is_err());
}

#[test]
fn config_rejects_duplicate_ids_and_nonzero_listen_addresses() {
    let mut duplicate_id = test_config();
    duplicate_id.channels.push(ChannelConfig {
        channel: channel_id("alpha"),
        enabled: false,
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        upstream_url: String::new(),
    });
    assert_eq!(
        duplicate_id.validate().unwrap_err().code,
        ErrorCode::ConfigInvalid.as_str()
    );

    let mut duplicate_address = test_config();
    duplicate_address.channels[0].listen_addr = "127.0.0.1:18080".parse().unwrap();
    duplicate_address.channels.push(ChannelConfig {
        channel: channel_id("gamma"),
        enabled: true,
        listen_addr: "127.0.0.1:18080".parse().unwrap(),
        upstream_url: "http://gamma.test/".into(),
    });
    assert_eq!(
        duplicate_address.validate().unwrap_err().code,
        ErrorCode::ConfigInvalid.as_str()
    );
}
