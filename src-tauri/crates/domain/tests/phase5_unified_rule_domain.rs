use std::collections::BTreeMap;

use intercept_proxy_domain::{
    BooleanPredicate, Condition, Document, DocumentMatchPath, DocumentMutation, DocumentNumber,
    DocumentPredicate, DocumentSchemaNode, DocumentValue, DocumentValueType, JsonPointer,
    NumberOperator, NumberPredicate, RuleId, RuleProgramEntry, StringOperator, StringPredicate,
    TerminalAction, UnifiedAction, UnifiedRuleProgram, document_condition_path_types,
    matches_document_conditions, validate_document_conditions_schema,
    validate_unified_actions_schema,
};
use uuid::Uuid;

fn path(value: &str) -> JsonPointer {
    JsonPointer::parse(value).expect("valid JSON pointer")
}

#[test]
fn clear_document_value_type_survives_strict_serde_round_trip() {
    let wire = serde_json::json!({
        "source": "document",
        "value": { "type": "clear", "path": "/enabled", "value_type": "boolean" }
    });
    let action: UnifiedAction = serde_json::from_value(wire.clone()).expect("typed clear");
    assert_eq!(serde_json::to_value(action).expect("serialize"), wire);
}

fn string_condition(path_value: &str, operator: StringOperator, value: &str) -> Condition {
    Condition::Document {
        path: path(path_value),
        predicate: DocumentPredicate::String(StringPredicate {
            operator,
            value: value.into(),
        }),
    }
}

fn entry(
    id: &str,
    priority: i32,
    created_order: u64,
    conditions: Vec<Condition>,
    actions: Vec<UnifiedAction>,
) -> RuleProgramEntry {
    RuleProgramEntry::new(
        RuleId::from_uuid(Uuid::parse_str(id).expect("uuid")),
        priority,
        created_order,
        conditions,
        actions,
    )
    .expect("valid program entry")
}

#[test]
fn flat_conditions_reject_empty_and_apply_fixed_and_semantics() {
    assert!(
        RuleProgramEntry::new(
            RuleId::new(),
            0,
            1,
            Vec::new(),
            vec![UnifiedAction::RecordMatch]
        )
        .is_err()
    );

    let conditions = vec![
        string_condition("/customer/name", StringOperator::StartsWith, "Ali"),
        Condition::Document {
            path: path("/customer/vip"),
            predicate: DocumentPredicate::Boolean(BooleanPredicate::Equal(true)),
        },
    ];

    let document = Document::parse_json(r#"{"customer":{"name":"Alice","age":17,"vip":true}}"#)
        .expect("document");
    assert!(matches_document_conditions(&conditions, &document).expect("match"));
}

#[test]
fn typed_predicate_operator_matrix_is_strict_and_missing_or_mismatch_is_false() {
    let document = Document::parse_json(r#"{"s":"prefix-value-suffix","n":10,"b":true,"z":null}"#)
        .expect("document");
    for operator in [
        StringOperator::Equal,
        StringOperator::Contains,
        StringOperator::StartsWith,
        StringOperator::EndsWith,
    ] {
        let expected = match operator {
            StringOperator::Equal => "prefix-value-suffix",
            StringOperator::Contains => "value",
            StringOperator::StartsWith => "prefix",
            StringOperator::EndsWith => "suffix",
        };
        assert!(
            matches_document_conditions(&[string_condition("/s", operator, expected)], &document)
                .expect("string predicate")
        );
    }
    for (operator, expected) in [
        (NumberOperator::Equal, 10.0),
        (NumberOperator::Less, 11.0),
        (NumberOperator::LessEqual, 10.0),
        (NumberOperator::Greater, 9.0),
        (NumberOperator::GreaterEqual, 10.0),
    ] {
        let condition = Condition::Document {
            path: path("/n"),
            predicate: DocumentPredicate::Number(NumberPredicate {
                operator,
                value: DocumentNumber::new(expected).expect("number"),
            }),
        };
        assert!(matches_document_conditions(&[condition], &document).expect("number predicate"));
    }
    assert!(
        matches_document_conditions(
            &[Condition::Document {
                path: path("/b"),
                predicate: DocumentPredicate::Boolean(BooleanPredicate::Equal(true)),
            }],
            &document
        )
        .expect("boolean")
    );
    assert!(
        matches_document_conditions(
            &[Condition::Document {
                path: path("/z"),
                predicate: DocumentPredicate::NullEqual,
            }],
            &document
        )
        .expect("null")
    );
    assert!(
        !matches_document_conditions(
            &[string_condition("/missing", StringOperator::Equal, "x")],
            &document
        )
        .expect("missing is false")
    );
    assert!(
        !matches_document_conditions(
            &[string_condition("/n", StringOperator::Equal, "10")],
            &document
        )
        .expect("type mismatch is false")
    );
}

#[test]
fn number_equal_uses_javascript_numeric_equality_for_signed_zero() {
    let positive_zero = Document::parse_json(r#"{"n":0}"#).expect("positive zero");
    let negative_zero = Document::new(DocumentValue::Number(
        DocumentNumber::new(-0.0).expect("negative zero"),
    ));
    let root_equal = |value| Condition::Document {
        path: path(""),
        predicate: DocumentPredicate::Number(NumberPredicate {
            operator: NumberOperator::Equal,
            value: DocumentNumber::new(value).expect("number"),
        }),
    };
    let field_equal = |value| Condition::Document {
        path: path("/n"),
        predicate: DocumentPredicate::Number(NumberPredicate {
            operator: NumberOperator::Equal,
            value: DocumentNumber::new(value).expect("number"),
        }),
    };

    assert!(matches_document_conditions(&[field_equal(-0.0)], &positive_zero).expect("-0 == +0"));
    assert!(matches_document_conditions(&[root_equal(0.0)], &negative_zero).expect("+0 == -0"));
    assert!(!matches_document_conditions(&[field_equal(1.0)], &positive_zero).expect("0 != 1"));
}

#[test]
fn large_flat_condition_and_action_lists_are_valid() {
    let leaf = string_condition("/value", StringOperator::Equal, "x");
    let conditions = vec![leaf; 1_025];

    RuleProgramEntry::new(
        RuleId::new(),
        0,
        1,
        conditions,
        vec![UnifiedAction::RecordMatch; 65],
    )
    .expect("65 actions are supported");
}

#[test]
fn schema_declared_paths_validate_and_undeclared_paths_keep_rule_local_type() {
    let schema = DocumentSchemaNode::Object {
        title: None,
        properties: BTreeMap::from([("amount".into(), DocumentSchemaNode::Number { title: None })]),
    };
    let declared_wrong = Condition::Document {
        path: path("/amount"),
        predicate: DocumentPredicate::String(StringPredicate {
            operator: StringOperator::Equal,
            value: "10".into(),
        }),
    };
    assert!(validate_document_conditions_schema(&[declared_wrong], &schema).is_err());

    let undeclared = Condition::Document {
        path: path("/custom"),
        predicate: DocumentPredicate::String(StringPredicate {
            operator: StringOperator::Equal,
            value: "local".into(),
        }),
    };
    validate_document_conditions_schema(std::slice::from_ref(&undeclared), &schema)
        .expect("undeclared path keeps predicate-local type");
    assert_eq!(
        document_condition_path_types(&[undeclared]),
        BTreeMap::from([(path("/custom"), DocumentValueType::String)])
    );
}

#[test]
fn schema_rejects_document_pattern_predicate_type_at_array_item_wildcard() {
    let schema = DocumentSchemaNode::Object {
        title: None,
        properties: BTreeMap::from([(
            "items".into(),
            DocumentSchemaNode::Array {
                title: None,
                items: Box::new(DocumentSchemaNode::String { title: None }),
            },
        )]),
    };
    let condition = Condition::DocumentPattern {
        path: DocumentMatchPath::parse("/items/*").expect("valid wildcard path"),
        predicate: DocumentPredicate::Number(NumberPredicate {
            operator: NumberOperator::Equal,
            value: DocumentNumber::new(1.0).expect("number"),
        }),
    };

    validate_document_conditions_schema(&[condition], &schema)
        .expect_err("array item schema is string, so a number predicate must be rejected");
}

#[test]
fn schema_rejects_clear_value_type_that_disagrees_with_declared_path() {
    let schema = DocumentSchemaNode::Object {
        title: None,
        properties: BTreeMap::from([("name".into(), DocumentSchemaNode::String { title: None })]),
    };
    let actions = [UnifiedAction::Document(DocumentMutation::Clear {
        path: path("/name"),
        value_type: DocumentValueType::Number,
    })];

    validate_unified_actions_schema(&actions, &schema)
        .expect_err("Clear metadata type must agree with the declared schema path");
}

#[test]
fn schema_enforces_insert_append_array_target_and_nested_item_type() {
    let schema = DocumentSchemaNode::Object {
        title: None,
        properties: BTreeMap::from([
            ("name".into(), DocumentSchemaNode::String { title: None }),
            (
                "matrix".into(),
                DocumentSchemaNode::Array {
                    title: None,
                    items: Box::new(DocumentSchemaNode::Array {
                        title: None,
                        items: Box::new(DocumentSchemaNode::Number { title: None }),
                    }),
                },
            ),
        ]),
    };
    let number = || DocumentValue::Number(DocumentNumber::new(1.0).expect("number"));

    validate_unified_actions_schema(
        &[UnifiedAction::Document(DocumentMutation::Insert {
            path: path("/name"),
            index: 0,
            value: DocumentValue::String("invalid target".into()),
        })],
        &schema,
    )
    .expect_err("Insert target declared as a string must be rejected");
    validate_unified_actions_schema(
        &[UnifiedAction::Document(DocumentMutation::Append {
            path: path("/name"),
            value: DocumentValue::String("invalid target".into()),
        })],
        &schema,
    )
    .expect_err("Append target declared as a string must be rejected");
    validate_unified_actions_schema(
        &[UnifiedAction::Document(DocumentMutation::Append {
            path: path("/matrix"),
            value: number(),
        })],
        &schema,
    )
    .expect_err("nested array target requires one complete array item operand");
    validate_unified_actions_schema(
        &[UnifiedAction::Document(DocumentMutation::Append {
            path: path("/matrix"),
            value: DocumentValue::Array(vec![number()]),
        })],
        &schema,
    )
    .expect("nested array target accepts an operand matching its array item schema");
}

#[test]
fn document_actions_apply_in_order_with_strict_set_clear_insert_and_append() {
    let rule = entry(
        "00000000-0000-0000-0000-000000000001",
        0,
        99,
        vec![string_condition("/status", StringOperator::Equal, "new")],
        vec![
            UnifiedAction::Document(DocumentMutation::Set {
                path: path("/status"),
                value: DocumentValue::String("ready".into()),
            }),
            UnifiedAction::Document(DocumentMutation::Insert {
                path: path("/items"),
                index: 1,
                value: DocumentValue::String("b".into()),
            }),
            UnifiedAction::Document(DocumentMutation::Append {
                path: path("/items"),
                value: DocumentValue::String("c".into()),
            }),
            UnifiedAction::Document(DocumentMutation::Clear {
                path: path("/remove"),
                value_type: DocumentValueType::Number,
            }),
        ],
    );
    let result = UnifiedRuleProgram::new(vec![rule])
        .expect("program")
        .execute(
            Document::parse_json(r#"{"status":"new","items":["a"],"remove":1}"#).expect("document"),
        )
        .expect("execution");
    assert_eq!(
        result.document().to_json().expect("json"),
        r#"{"items":["a","b","c"],"status":"ready"}"#
    );
}

#[test]
fn runtime_order_is_priority_then_rule_id_and_later_rules_see_working_mutation() {
    let later_id_first = entry(
        "00000000-0000-0000-0000-000000000001",
        1,
        999,
        vec![string_condition("/state", StringOperator::Equal, "initial")],
        vec![UnifiedAction::Document(DocumentMutation::Set {
            path: path("/state"),
            value: DocumentValue::String("changed".into()),
        })],
    );
    let later_id_second = entry(
        "00000000-0000-0000-0000-000000000002",
        1,
        1,
        vec![string_condition("/state", StringOperator::Equal, "changed")],
        vec![UnifiedAction::Document(DocumentMutation::Set {
            path: path("/seen"),
            value: DocumentValue::Boolean(true),
        })],
    );
    let result = UnifiedRuleProgram::new(vec![later_id_second, later_id_first])
        .expect("program")
        .execute(Document::parse_json(r#"{"state":"initial"}"#).expect("document"))
        .expect("execution");
    assert_eq!(result.matched_rule_ids().len(), 2);
    assert_eq!(
        result.document().resolve(&path("/seen")).expect("seen"),
        &DocumentValue::Boolean(true)
    );
}

#[test]
fn terminal_action_must_be_unique_and_last_and_stops_later_rules() {
    let conditions = vec![string_condition("/state", StringOperator::Equal, "initial")];
    assert!(
        RuleProgramEntry::new(
            RuleId::new(),
            0,
            1,
            conditions.clone(),
            vec![
                UnifiedAction::Terminal(TerminalAction::DisconnectBeforeUpstream),
                UnifiedAction::Document(DocumentMutation::Set {
                    path: path("/state"),
                    value: DocumentValue::String("invalid".into()),
                }),
            ],
        )
        .is_err()
    );

    let terminal = entry(
        "00000000-0000-0000-0000-000000000001",
        0,
        1,
        conditions,
        vec![UnifiedAction::Terminal(
            TerminalAction::DisconnectBeforeUpstream,
        )],
    );
    let never_runs = entry(
        "00000000-0000-0000-0000-000000000002",
        1,
        2,
        vec![string_condition("/state", StringOperator::Equal, "initial")],
        vec![UnifiedAction::Document(DocumentMutation::Set {
            path: path("/state"),
            value: DocumentValue::String("wrong".into()),
        })],
    );
    let result = UnifiedRuleProgram::new(vec![never_runs, terminal])
        .expect("program")
        .execute(Document::parse_json(r#"{"state":"initial"}"#).expect("document"))
        .expect("terminal is a successful domain decision");
    assert_eq!(
        result.terminal_action(),
        Some(&TerminalAction::DisconnectBeforeUpstream)
    );
    assert_eq!(result.matched_rule_ids().len(), 1);
}

#[test]
fn cloned_rule_configuration_is_deeply_independent() {
    let original = entry(
        "00000000-0000-0000-0000-000000000001",
        0,
        1,
        vec![string_condition("/name", StringOperator::Equal, "original")],
        vec![UnifiedAction::Document(DocumentMutation::Set {
            path: path("/name"),
            value: DocumentValue::String("changed".into()),
        })],
    );
    let mut copied = original.clone();
    copied
        .replace_conditions(vec![string_condition(
            "/name",
            StringOperator::Equal,
            "copy",
        )])
        .expect("replacement remains validated");
    assert_ne!(original.conditions(), copied.conditions());

    assert!(copied.replace_conditions(Vec::new()).is_err());
    assert!(UnifiedRuleProgram::new(vec![original.clone(), original]).is_err());
}
