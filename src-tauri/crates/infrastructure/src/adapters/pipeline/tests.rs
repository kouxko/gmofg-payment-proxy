include!("tests/support.rs");
include!("tests/wire_mutation.rs");
include!("tests/runtime_support.rs");
include!("tests/session_runtime.rs");
include!("tests/breakpoints.rs");
include!("tests/rules_and_faults.rs");

#[path = "tests/http_protocol.rs"]
mod http_protocol_tests;
