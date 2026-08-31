use crate::{
    AppError, AppResult, RuleLocalDocumentActionKind, RuleLocalDocumentPredicateKind,
    RuleLocalDocumentValueType,
};
use intercept_proxy_domain::{
    BooleanPredicate, Condition, Document, DocumentMutation, DocumentPredicate, DocumentValue,
    JsonPointer, NumberOperator, NumberPredicate, StringOperator, StringPredicate, UnifiedAction,
};

pub(super) fn condition_draft(
    path: &str,
    value_type: RuleLocalDocumentValueType,
    predicate: RuleLocalDocumentPredicateKind,
    raw: &str,
) -> AppResult<Condition> {
    let value = parse_value(value_type, raw)?;
    let predicate = match (value, predicate) {
        (DocumentValue::String(value), RuleLocalDocumentPredicateKind::Equals) => {
            string(StringOperator::Equal, value)
        }
        (DocumentValue::String(value), RuleLocalDocumentPredicateKind::Contains) => {
            string(StringOperator::Contains, value)
        }
        (DocumentValue::String(value), RuleLocalDocumentPredicateKind::StartsWith) => {
            string(StringOperator::StartsWith, value)
        }
        (DocumentValue::String(value), RuleLocalDocumentPredicateKind::EndsWith) => {
            string(StringOperator::EndsWith, value)
        }
        (DocumentValue::Number(value), RuleLocalDocumentPredicateKind::Equals) => {
            number(NumberOperator::Equal, value)
        }
        (DocumentValue::Number(value), RuleLocalDocumentPredicateKind::Less) => {
            number(NumberOperator::Less, value)
        }
        (DocumentValue::Number(value), RuleLocalDocumentPredicateKind::LessEqual) => {
            number(NumberOperator::LessEqual, value)
        }
        (DocumentValue::Number(value), RuleLocalDocumentPredicateKind::Greater) => {
            number(NumberOperator::Greater, value)
        }
        (DocumentValue::Number(value), RuleLocalDocumentPredicateKind::GreaterEqual) => {
            number(NumberOperator::GreaterEqual, value)
        }
        (DocumentValue::Boolean(value), RuleLocalDocumentPredicateKind::Equals) => {
            DocumentPredicate::Boolean(BooleanPredicate::Equal(value))
        }
        (DocumentValue::Null(()), RuleLocalDocumentPredicateKind::Equals) => {
            DocumentPredicate::NullEqual
        }
        _ => return Err(invalid_capability("predicate")),
    };
    Ok(Condition::Document {
        path: JsonPointer::parse(path)?,
        predicate,
    })
}

pub(super) fn action_draft(
    path: &str,
    value_type: RuleLocalDocumentValueType,
    action: RuleLocalDocumentActionKind,
    raw: Option<&str>,
    index: Option<u32>,
) -> AppResult<UnifiedAction> {
    let path = JsonPointer::parse(path)?;
    let mutation = match action {
        RuleLocalDocumentActionKind::Clear => DocumentMutation::Clear {
            path,
            value_type: domain_value_type(value_type),
        },
        RuleLocalDocumentActionKind::Set => DocumentMutation::Set {
            path,
            value: required_value(value_type, raw)?,
        },
        RuleLocalDocumentActionKind::Insert => DocumentMutation::Insert {
            path,
            index: index
                .ok_or_else(|| AppError::new("RULE_INVALID", "Insert 动作需要显式 index。"))?
                as usize,
            value: required_value(value_type, raw)?,
        },
        RuleLocalDocumentActionKind::Append => DocumentMutation::Append {
            path,
            value: required_value(value_type, raw)?,
        },
    };
    Ok(UnifiedAction::Document(mutation))
}

fn string(operator: StringOperator, value: String) -> DocumentPredicate {
    DocumentPredicate::String(StringPredicate { operator, value })
}

fn number(
    operator: NumberOperator,
    value: intercept_proxy_domain::DocumentNumber,
) -> DocumentPredicate {
    DocumentPredicate::Number(NumberPredicate { operator, value })
}

fn required_value(
    value_type: RuleLocalDocumentValueType,
    raw: Option<&str>,
) -> AppResult<DocumentValue> {
    parse_value(
        value_type,
        raw.ok_or_else(|| AppError::new("RULE_INVALID", "该动作需要显式 JSON 值。"))?,
    )
}

const fn domain_value_type(
    value_type: RuleLocalDocumentValueType,
) -> intercept_proxy_domain::DocumentValueType {
    match value_type {
        RuleLocalDocumentValueType::String => intercept_proxy_domain::DocumentValueType::String,
        RuleLocalDocumentValueType::Number => intercept_proxy_domain::DocumentValueType::Number,
        RuleLocalDocumentValueType::Boolean => intercept_proxy_domain::DocumentValueType::Boolean,
        RuleLocalDocumentValueType::Null => intercept_proxy_domain::DocumentValueType::Null,
        RuleLocalDocumentValueType::Object => intercept_proxy_domain::DocumentValueType::Object,
        RuleLocalDocumentValueType::Array => intercept_proxy_domain::DocumentValueType::Array,
    }
}

fn parse_value(expected: RuleLocalDocumentValueType, raw: &str) -> AppResult<DocumentValue> {
    let value = Document::parse_json(raw)?.root().clone();
    let actual = match value.value_type() {
        intercept_proxy_domain::DocumentValueType::String => RuleLocalDocumentValueType::String,
        intercept_proxy_domain::DocumentValueType::Number => RuleLocalDocumentValueType::Number,
        intercept_proxy_domain::DocumentValueType::Boolean => RuleLocalDocumentValueType::Boolean,
        intercept_proxy_domain::DocumentValueType::Null => RuleLocalDocumentValueType::Null,
        intercept_proxy_domain::DocumentValueType::Object => RuleLocalDocumentValueType::Object,
        intercept_proxy_domain::DocumentValueType::Array => RuleLocalDocumentValueType::Array,
    };
    if actual != expected {
        return Err(AppError::new(
            "RULE_INVALID",
            "JSON 值类型与显式选择的 Document 类型不一致。",
        ));
    }
    Ok(value)
}

fn invalid_capability(field: &'static str) -> AppError {
    AppError::new(
        "RULE_INVALID",
        format!("所选 Document 类型不支持该 {field} 能力。"),
    )
}

#[cfg(test)]
mod tests {
    use super::{action_draft, condition_draft};
    use crate::{
        RuleLocalDocumentActionKind, RuleLocalDocumentPredicateKind, RuleLocalDocumentValueType,
    };

    #[test]
    fn builds_null_object_and_array_leaves_without_ui_defaults() {
        let condition = condition_draft(
            "/nullable",
            RuleLocalDocumentValueType::Null,
            RuleLocalDocumentPredicateKind::Equals,
            "null",
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(condition).unwrap()["predicate"]["type"],
            "null_equal"
        );
        let object = action_draft(
            "/metadata",
            RuleLocalDocumentValueType::Object,
            RuleLocalDocumentActionKind::Set,
            Some(r#"{"merchant":"m-1"}"#),
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(object).unwrap()["value"]["value"]["merchant"],
            "m-1"
        );
        let array = action_draft(
            "/items",
            RuleLocalDocumentValueType::Array,
            RuleLocalDocumentActionKind::Set,
            Some("[1,null]"),
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(array).unwrap()["value"]["value"][1],
            serde_json::Value::Null
        );
        let clear = action_draft(
            "/enabled",
            RuleLocalDocumentValueType::Boolean,
            RuleLocalDocumentActionKind::Clear,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(clear).unwrap()["value"]["value_type"],
            "boolean"
        );
    }
}
