use std::{collections::BTreeMap, sync::Arc};

use crate::{
    PackageFilePath, ProtocolExecutionCancellation, ProtocolPackageCompiler, ProtocolPackageFiles,
    ProtocolRuntimeLimits, host::context::ProtocolDirection,
};

use super::{
    ProtocolFrameInspection, ProtocolFrameInspector, ProtocolFramingError,
    ProtocolFramingErrorCode, ProtocolFramingLimit, ProtocolFramingLimits, ProtocolReader,
};

const DOCUMENT_SCHEMA: &str = r#"type = "object"
title = "Framing Test"

[properties.kind]
type = "number"
title = "Kind"
"#;

include!("tests/reader_and_limits.rs");
include!("tests/inspector.rs");
include!("tests/rhai.rs");
include!("tests/support.rs");
