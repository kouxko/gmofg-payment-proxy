use std::collections::BTreeMap;

use intercept_proxy_domain::{
    BooleanPredicate, Condition, ConditionTree, Document, DocumentMutation, DocumentNumber,
    DocumentPredicate, DocumentSchemaNode, DocumentValue, DocumentValueType, JsonPointer,
    NumberOperator, NumberPredicate, RuleId, RuleProgramEntry, StringOperator, StringPredicate,
    TerminalAction, UnifiedAction, UnifiedRuleProgram,
};
use uuid::Uuid;

fn path(value: &str) -> JsonPointer {
    JsonPointer::parse(value).expect("valid JSON pointer")
}

fn string_condition(path_value: &str, operator: StringOperator, value: &str) -> ConditionTree {
    ConditionTree::Leaf(Condition::Document {
        path: path(path_value),
        predicate: DocumentPredicate::String(StringPredicate {
            operator,
            value: value.into(),
        }),
    })
}

fn entry(
    id: &str,
    priority: i32,
    created_order: u64,
    condition: ConditionTree,
    actions: Vec<UnifiedAction>,
) -> RuleProgramEntry {
    RuleProgramEntry::new(
        RuleId::from_uuid(Uuid::parse_str(id).expect("uuid")),
        priority,
        created_order,
        condition,
        actions,
    )
    .expect("valid program entry")
}

#[test]
fn condition_tree_rejects_empty_groups_and_supports_nested_and_or() {
    assert!(ConditionTree::all(Vec::new()).is_err());
    assert!(ConditionTree::any(Vec::new()).is_err());

    let condition = ConditionTree::all(vec![
        string_condition("/customer/name", StringOperator::StartsWith, "Ali"),
        ConditionTree::any(vec![
            ConditionTree::Leaf(Condition::Document {
                path: path("/customer/age"),
                predicate: DocumentPredicate::Number(NumberPredicate {
                    operator: NumberOperator::GreaterEqual,
                    value: DocumentNumber::new(18.0).expect("number"),
                }),
            }),
            ConditionTree::Leaf(Condition::Document {
                path: path("/customer/vip"),
                predicate: DocumentPredicate::Boolean(BooleanPredicate::Equal(true)),
            }),
        ])
        .expect("non-empty OR"),
    ])
    .expect("non-empty AND");

    let document = Document::parse_json(r#"{"customer":{"name":"Alice","age":17,"vip":true}}"#)
        .expect("document");
    assert!(condition.matches_document(&document).expect("match"));
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
            string_condition("/s", operator, expected)
                .matches_document(&document)
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
        let condition = ConditionTree::Leaf(Condition::Document {
            path: path("/n"),
            predicate: DocumentPredicate::Number(NumberPredicate {
                operator,
                value: DocumentNumber::new(expected).expect("number"),
            }),
        });
        assert!(
            condition
                .matches_document(&document)
                .expect("number predicate")
        );
    }
    assert!(
        ConditionTree::Leaf(Condition::Document {
            path: path("/b"),
            predicate: DocumentPredicate::Boolean(BooleanPredicate::Equal(true)),
        })
        .matches_document(&document)
        .expect("boolean")
    );
    assert!(
        ConditionTree::Leaf(Condition::Document {
            path: path("/z"),
            predicate: DocumentPredicate::NullEqual,
        })
        .matches_document(&document)
        .expect("null")
    );
    assert!(
        !string_condition("/missing", StringOperator::Equal, "x")
            .matches_document(&document)
            .expect("missing is false")
    );
    assert!(
        !string_condition("/n", StringOperator::Equal, "10")
            .matches_document(&document)
            .expect("type mismatch is false")
    );
}

#[test]
fn number_equal_uses_javascript_numeric_equality_for_signed_zero() {
    let positive_zero = Document::parse_json(r#"{"n":0}"#).expect("positive zero");
    let negative_zero = Document::new(DocumentValue::Number(
        DocumentNumber::new(-0.0).expect("negative zero"),
    ));
    let root_equal = |value| {
        ConditionTree::Leaf(Condition::Document {
            path: path(""),
            predicate: DocumentPredicate::Number(NumberPredicate {
                operator: NumberOperator::Equal,
                value: DocumentNumber::new(value).expect("number"),
            }),
        })
    };
    let field_equal = |value| {
        ConditionTree::Leaf(Condition::Document {
            path: path("/n"),
            predicate: DocumentPredicate::Number(NumberPredicate {
                operator: NumberOperator::Equal,
                value: DocumentNumber::new(value).expect("number"),
            }),
        })
    };

    assert!(
        field_equal(-0.0)
            .matches_document(&positive_zero)
            .expect("-0 == +0")
    );
    assert!(
        root_equal(0.0)
            .matches_document(&negative_zero)
            .expect("+0 == -0")
    );
    assert!(
        !field_equal(1.0)
            .matches_document(&positive_zero)
            .expect("0 != 1")
    );
}

#[test]
fn arbitrary_nested_nonempty_trees_and_long_action_lists_are_valid() {
    let leaf = string_condition("/value", StringOperator::Equal, "x");
    let depth_65 = (0..65).fold(leaf.clone(), |tree, _| ConditionTree::All(vec![tree]));
    depth_65.validate().expect("65 nested groups are supported");

    let nodes_1025 = ConditionTree::All(vec![leaf; 1_025]);
    nodes_1025
        .validate()
        .expect("1025 leaf nodes are supported");

    RuleProgramEntry::new(
        RuleId::new(),
        0,
        1,
        depth_65,
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
    let declared_wrong = ConditionTree::Leaf(Condition::Document {
        path: path("/amount"),
        predicate: DocumentPredicate::String(StringPredicate {
            operator: StringOperator::Equal,
            value: "10".into(),
        }),
    });
    assert!(declared_wrong.validate_document_schema(&schema).is_err());

    let undeclared = ConditionTree::Leaf(Condition::Document {
        path: path("/custom"),
        predicate: DocumentPredicate::String(StringPredicate {
            operator: StringOperator::Equal,
            value: "local".into(),
        }),
    });
    undeclared
        .validate_document_schema(&schema)
        .expect("undeclared path keeps predicate-local type");
    assert_eq!(
        undeclared.document_path_types(),
        BTreeMap::from([(path("/custom"), DocumentValueType::String)])
    );
}

#[test]
fn document_actions_apply_in_order_with_strict_set_clear_insert_and_append() {
    let rule = entry(
        "00000000-0000-0000-0000-000000000001",
        0,
        99,
        string_condition("/status", StringOperator::Equal, "new"),
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
        string_condition("/state", StringOperator::Equal, "initial"),
        vec![UnifiedAction::Document(DocumentMutation::Set {
            path: path("/state"),
            value: DocumentValue::String("changed".into()),
        })],
    );
    let later_id_second = entry(
        "00000000-0000-0000-0000-000000000002",
        1,
        1,
        string_condition("/state", StringOperator::Equal, "changed"),
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
    let condition = string_condition("/state", StringOperator::Equal, "initial");
    assert!(
        RuleProgramEntry::new(
            RuleId::new(),
            0,
            1,
            condition.clone(),
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
        condition,
        vec![UnifiedAction::Terminal(
            TerminalAction::DisconnectBeforeUpstream,
        )],
    );
    let never_runs = entry(
        "00000000-0000-0000-0000-000000000002",
        1,
        2,
        string_condition("/state", StringOperator::Equal, "initial"),
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
        string_condition("/name", StringOperator::Equal, "original"),
        vec![UnifiedAction::Document(DocumentMutation::Set {
            path: path("/name"),
            value: DocumentValue::String("changed".into()),
        })],
    );
    let mut copied = original.clone();
    copied
        .replace_condition(string_condition("/name", StringOperator::Equal, "copy"))
        .expect("replacement remains validated");
    assert_ne!(original.condition(), copied.condition());

    assert!(
        copied
            .replace_condition(ConditionTree::All(Vec::new()))
            .is_err()
    );
    assert!(UnifiedRuleProgram::new(vec![original.clone(), original]).is_err());
}
