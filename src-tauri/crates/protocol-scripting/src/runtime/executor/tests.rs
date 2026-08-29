use rhai::{Dynamic, EvalAltResult, Position};

use crate::ProtocolResourceLimit;

use super::find_resource_limit;

#[test]
fn nested_rhai_resource_errors_map_without_using_display_text() {
    let string = EvalAltResult::ErrorDataTooLarge("Length of string".to_owned(), Position::NONE);
    let nested = EvalAltResult::ErrorInFunctionCall(
        "helper".to_owned(),
        "safe-source".to_owned(),
        Box::new(string),
        Position::NONE,
    );
    assert_eq!(
        find_resource_limit(&nested),
        Some(ProtocolResourceLimit::StringBytes)
    );

    let nested_module = EvalAltResult::ErrorInModule(
        "module".to_owned(),
        Box::new(EvalAltResult::ErrorTerminated(
            Dynamic::UNIT,
            Position::NONE,
        )),
        Position::NONE,
    );
    assert_eq!(
        find_resource_limit(&nested_module),
        Some(ProtocolResourceLimit::WallTimeMs)
    );
    assert_eq!(
        find_resource_limit(&EvalAltResult::ErrorRuntime(Dynamic::UNIT, Position::NONE)),
        None
    );
}
