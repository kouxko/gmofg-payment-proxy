use std::collections::BTreeMap;

use intercept_proxy_domain::DocumentSchemaNode;
use rhai::Engine;

use crate::{
    ProtocolDirection, ProtocolRuntimeLimits, compiler::build_engine, host::ProtocolHostApi,
    test_support::CompiledProtocolPackageTestBuilder,
};

pub(super) fn schema() -> DocumentSchemaNode {
    DocumentSchemaNode::Object {
        title: Some("Host Test Message".to_owned()),
        properties: BTreeMap::from([
            (
                "text".to_owned(),
                DocumentSchemaNode::String { title: None },
            ),
            (
                "number".to_owned(),
                DocumentSchemaNode::Number { title: None },
            ),
            (
                "flag".to_owned(),
                DocumentSchemaNode::Boolean { title: None },
            ),
            (
                "nested".to_owned(),
                DocumentSchemaNode::Object {
                    title: None,
                    properties: BTreeMap::from([(
                        "items".to_owned(),
                        DocumentSchemaNode::Array {
                            title: None,
                            items: Box::new(DocumentSchemaNode::Number { title: None }),
                        },
                    )]),
                },
            ),
        ]),
    }
}

pub(super) fn engine() -> Engine {
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_schema(schema())
        .build();
    let host = ProtocolHostApi::for_package(&package, ProtocolDirection::Upstream);
    let mut engine = build_engine(ProtocolRuntimeLimits::default());
    host.register(&mut engine);
    engine
}
