use super::{
    CompiledDirection, CompiledProtocolPackage, Document, EvalAltResult, ProtocolDirection,
    ProtocolResourceLimit,
};

pub(super) fn compiled_direction(
    package: &CompiledProtocolPackage,
    direction: ProtocolDirection,
) -> &CompiledDirection {
    match direction {
        ProtocolDirection::Upstream => package.upstream(),
        ProtocolDirection::Downstream => package.downstream(),
    }
}

pub(super) fn exceeds_limit(length: usize, limit: u64) -> bool {
    u64::try_from(length).map_or(true, |length| length > limit)
}

pub(super) fn validate_document_schema(
    document: &Document,
    expected: &intercept_proxy_domain::DocumentSchema,
) -> Result<(), ()> {
    if document.schema() != expected {
        return Err(());
    }
    // Document 的字段槽只能经类型安全 Domain API 或已校验反序列化创建；遍历仍在执行边界重新
    // 核对，避免未来新增构造路径时让错误字段类型进入规则或 Encode。
    if document.fields().any(|state| {
        state
            .value
            .is_some_and(|value| value.field_type() != state.field.field_type())
    }) {
        return Err(());
    }
    Ok(())
}

pub(super) fn find_resource_limit(error: &EvalAltResult) -> Option<ProtocolResourceLimit> {
    match error {
        EvalAltResult::ErrorTooManyOperations(_) => Some(ProtocolResourceLimit::Operations),
        EvalAltResult::ErrorStackOverflow(_) => Some(ProtocolResourceLimit::CallDepth),
        EvalAltResult::ErrorDataTooLarge(kind, _)
            if kind.to_ascii_lowercase().contains("string") =>
        {
            Some(ProtocolResourceLimit::StringBytes)
        }
        EvalAltResult::ErrorDataTooLarge(_, _) => Some(ProtocolResourceLimit::BlobBytes),
        EvalAltResult::ErrorTerminated(_, _) => Some(ProtocolResourceLimit::WallTimeMs),
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _)
        | EvalAltResult::ErrorInModule(_, inner, _) => find_resource_limit(inner),
        _ => None,
    }
}
