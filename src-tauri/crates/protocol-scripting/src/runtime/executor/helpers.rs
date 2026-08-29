use super::{
    CompiledDirection, CompiledProtocolPackage, EvalAltResult, ProtocolDirection,
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
