use std::{collections::BTreeMap, sync::Arc};

use crate::{
    PackageFilePath, ProtocolExecutionCancellation, ProtocolPackageCompiler, ProtocolPackageFiles,
    ProtocolRuntimeLimits, host::context::ProtocolDirection,
};

use super::{
    FrameBuffer, FramingDecision, ProtocolFrameInspection, ProtocolFrameInspector,
    ProtocolFramingError, ProtocolFramingErrorCode, ProtocolFramingLimit, ProtocolFramingLimits,
    ProtocolReader, ReaderSegment, framer::SingleDirectionFramer, script::RhaiFrameDecider,
};

const DOCUMENT_SCHEMA: &str = r#"id = "framing-test"
version = 1
title = "Framing Test"

[[fields]]
name = "kind"
label = "Kind"
type = "int"
"#;

include!("tests/reader_and_limits.rs");
include!("tests/framer.rs");
include!("tests/inspector.rs");
include!("tests/rhai.rs");
include!("tests/support.rs");
