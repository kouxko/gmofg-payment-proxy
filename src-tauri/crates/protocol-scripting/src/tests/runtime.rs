use std::sync::atomic::{AtomicUsize, Ordering};

use intercept_proxy_domain::{
    DirectionProcessingOptions, Document, DocumentField, DocumentFieldName, DocumentFieldType,
    DocumentSchema, DocumentSchemaId, DocumentValue,
};

use crate::{
    DirectionExecutionPlan, DisplayFallbackReason, ProtocolDirection, ProtocolDirectionExecutor,
    ProtocolDisplayResult, ProtocolEntryPoint, ProtocolResourceLimit, ProtocolRuntimeError,
    ProtocolRuntimeLimits, test_support::CompiledProtocolPackageTestBuilder,
};

const VALID_SCRIPT: &str = r#"
fn frame(reader, context) { () }

fn decode(origin, context) {
    if context.stage() != "receive" { throw "wrong decode stage"; }
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("amount", 7);
    } else {
        value.set("amount", 8);
    }
    value
}

fn encode(origin, document, context) {
    if context.stage() != "send" { throw "wrong encode stage"; }
    let result = blob(2, 0);
    result[0] = if context.direction() == "upstream" { 0x55 } else { 0x44 };
    result[1] = if document.has("amount") { document.get("amount") } else { 0 };
    result
}

fn display(document, context) {
    if context.stage() != "display" { throw "wrong display stage"; }
    if context.direction() == "upstream" { "upstream-html" } else { "downstream-html" }
}
"#;

fn options(decode_enabled: bool, encode_enabled: bool) -> DirectionProcessingOptions {
    DirectionProcessingOptions {
        decode_enabled,
        encode_enabled,
    }
}

fn package_with_all_entries() -> crate::CompiledProtocolPackage {
    CompiledProtocolPackageTestBuilder::new()
        .with_script(VALID_SCRIPT)
        .with_upstream_encode()
        .with_downstream_encode()
        .with_display()
        .build()
}

fn executor(
    package: &crate::CompiledProtocolPackage,
    direction: ProtocolDirection,
    decode_enabled: bool,
    encode_enabled: bool,
) -> ProtocolDirectionExecutor {
    let plan =
        DirectionExecutionPlan::new(package, direction, options(decode_enabled, encode_enabled))
            .unwrap();
    ProtocolDirectionExecutor::new(
        package,
        plan,
        "connection-1",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap()
}

include!("runtime/execution.rs");
include!("runtime/resource_limits.rs");
include!("runtime/contracts.rs");
