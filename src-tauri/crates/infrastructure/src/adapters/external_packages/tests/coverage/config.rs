use std::time::Duration;

use crate::adapters::external_packages::ExternalPackageConnectionConfig;

const ONE_SECOND: Duration = Duration::from_secs(1);

fn config_with(
    registration_timeout: Duration,
    rpc_timeout: Duration,
    heartbeat_interval: Duration,
    heartbeat_timeout: Duration,
    limits: [usize; 5],
) -> ExternalPackageConnectionConfig {
    ExternalPackageConnectionConfig::new(
        registration_timeout,
        rpc_timeout,
        heartbeat_interval,
        heartbeat_timeout,
        limits[0],
        limits[1],
        limits[2],
        limits[3],
        limits[4],
    )
}

#[test]
fn client_config_exposes_all_configured_limits() {
    let config = config_with(
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(3),
        Duration::from_secs(4),
        [5, 6, 7, 8, 9],
    );

    assert_eq!(config.max_in_flight(), 5);
    assert_eq!(config.max_logical_frame_bytes(), 6);
    assert_eq!(config.max_registration_message_bytes(), 7);
    assert_eq!(config.max_rpc_message_bytes(), 8);
    assert_eq!(config.max_display_message_bytes(), 9);
    assert_eq!(config.rpc_timeout(), Duration::from_secs(2));
    assert_eq!(config.registration_websocket_message_bytes(), 7);
    assert_eq!(config.write_timeout(), Duration::from_secs(2));
}

#[test]
#[should_panic(expected = "max_in_flight must be positive")]
fn config_rejects_zero_in_flight_limit() {
    let _ = config_with(
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        [0, 1, 1, 1, 1],
    );
}

#[test]
#[should_panic(expected = "logical frame limit must be positive")]
fn config_rejects_zero_logical_frame_limit() {
    let _ = config_with(
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        [1, 0, 1, 1, 1],
    );
}

#[test]
#[should_panic(expected = "registration limit must be positive")]
fn config_rejects_zero_registration_limit() {
    let _ = config_with(
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        [1, 1, 0, 1, 1],
    );
}

#[test]
#[should_panic(expected = "RPC limit must be positive")]
fn config_rejects_zero_rpc_limit() {
    let _ = config_with(
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        [1, 1, 1, 0, 1],
    );
}

#[test]
#[should_panic(expected = "display limit must be positive")]
fn config_rejects_zero_display_limit() {
    let _ = config_with(
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        ONE_SECOND,
        [1, 1, 1, 1, 0],
    );
}

#[test]
#[should_panic(expected = "registration timeout must be positive")]
fn config_rejects_zero_registration_timeout() {
    let _ = config_with(Duration::ZERO, ONE_SECOND, ONE_SECOND, ONE_SECOND, [1; 5]);
}

#[test]
#[should_panic(expected = "RPC timeout must be positive")]
fn config_rejects_zero_rpc_timeout() {
    let _ = config_with(ONE_SECOND, Duration::ZERO, ONE_SECOND, ONE_SECOND, [1; 5]);
}

#[test]
#[should_panic(expected = "heartbeat interval must be positive")]
fn config_rejects_zero_heartbeat_interval() {
    let _ = config_with(ONE_SECOND, ONE_SECOND, Duration::ZERO, ONE_SECOND, [1; 5]);
}

#[test]
#[should_panic(expected = "heartbeat timeout must cover one interval")]
fn config_rejects_heartbeat_timeout_shorter_than_interval() {
    let _ = config_with(
        ONE_SECOND,
        ONE_SECOND,
        Duration::from_secs(2),
        ONE_SECOND,
        [1; 5],
    );
}
