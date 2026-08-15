include!("tests/certificate_policy.rs");
include!("tests/forward_proxy.rs");
include!("tests/fixed_server.rs");
include!("tests/socket_runtime.rs");
include!("tests/validation.rs");

#[path = "tests/scripted_relay_runtime.rs"]
mod scripted_relay_runtime_tests;
#[path = "tests/scripted_snapshot.rs"]
mod scripted_snapshot_tests;
