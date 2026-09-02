//! Phase 5 unified rule condition, action, validation, and deterministic execution primitives.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Document, DocumentMatchPath, DocumentSchemaNode, DocumentValue, DocumentValueType, DomainError,
    ErrorCode, HttpAction, JsonPointer, MatchContext, MatchField, MatchOperator, RuleId,
    TerminalAction, matches_http_condition,
};

mod condition_evaluation;
mod mutation;
mod program;

pub use condition_evaluation::{ConditionEvaluation, RuleConditionEvaluation};
pub use mutation::validate_unified_action_schema;
pub use program::{RuleProgramEntry, UnifiedRuleExecution, UnifiedRuleProgram};

/// String comparison supported by a typed Document predicate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StringOperator {
    /// Exact string equality.
    Equal,
    /// Substring containment.
    Contains,
    /// Prefix match.
    StartsWith,
    /// Suffix match.
    EndsWith,
}

/// A string predicate whose expected value cannot be confused with another JSON type.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct StringPredicate {
    /// Comparison operator.
    pub operator: StringOperator,
    /// Expected string.
    pub value: String,
}

/// Number comparison supported by a typed Document predicate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NumberOperator {
    /// Exact numeric equality.
    Equal,
    /// Strictly less than.
    Less,
    /// Less than or equal.
    LessEqual,
    /// Strictly greater than.
    Greater,
    /// Greater than or equal.
    GreaterEqual,
}

/// A number predicate whose expected value is a validated JavaScript Number.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct NumberPredicate {
    /// Comparison operator.
    pub operator: NumberOperator,
    /// Expected number.
    pub value: crate::DocumentNumber,
}

/// Boolean equality predicate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BooleanPredicate {
    /// Exact boolean equality.
    Equal(bool),
}

/// Closed, typed Document predicate set. No implicit JSON value conversions are performed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DocumentPredicate {
    /// String predicate.
    String(StringPredicate),
    /// Number predicate.
    Number(NumberPredicate),
    /// Boolean predicate.
    Boolean(BooleanPredicate),
    /// Exact JSON null equality.
    NullEqual,
}

/// The rule's one typed condition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Condition {
    /// A typed RFC 6901 Document predicate.
    Document {
        /// Document path.
        path: JsonPointer,
        /// Strict predicate.
        predicate: DocumentPredicate,
    },
    /// A typed Document predicate whose condition-only path may contain `*` segments.
    DocumentPattern {
        /// Condition-only match path.
        path: DocumentMatchPath,
        /// Strict predicate applied with ANY semantics to expanded values.
        predicate: DocumentPredicate,
    },
    /// Existing typed HTTP/runtime condition.
    Http {
        /// Typed HTTP field.
        field: MatchField,
        /// Typed HTTP comparison.
        operator: MatchOperator,
    },
}

#[must_use]
pub const fn is_document_condition(condition: &Condition) -> bool {
    matches!(
        condition,
        Condition::Document { .. } | Condition::DocumentPattern { .. }
    )
}

/// Evaluates one condition using a caller-provided evaluator for typed HTTP conditions.
pub fn evaluate_condition<E>(
    condition: &Condition,
    document: &Document,
    http_matches: &mut impl FnMut(&MatchField, &MatchOperator) -> Result<bool, E>,
) -> Result<ConditionEvaluation, E>
where
    E: From<DomainError>,
{
    let evaluated = match condition {
        Condition::Document { path, predicate } => match document.resolve(path) {
            Ok(actual) => ConditionEvaluation::ordinary(predicate.matches(actual)),
            Err(DomainError {
                code: ErrorCode::DocumentPathMissing | ErrorCode::DocumentPathTypeMismatch,
                ..
            }) => ConditionEvaluation::ordinary(false),
            Err(error) => return Err(error.into()),
        },
        Condition::DocumentPattern { path, predicate } => ConditionEvaluation::ordinary(
            document
                .resolve_match_path(path)
                .into_iter()
                .any(|actual| predicate.matches(actual)),
        ),
        Condition::Http { field, operator } => {
            ConditionEvaluation::ordinary(http_matches(field, operator)?)
        }
    };
    Ok(evaluated)
}

/// Matches one Document condition. HTTP conditions are rejected at this boundary.
pub fn matches_document_condition(
    condition: &Condition,
    document: &Document,
) -> Result<bool, DomainError> {
    Ok(evaluate_condition(condition, document, &mut |_, _| {
        Err(rule_error(
            "condition",
            "HTTP 条件需要应用层提供类型化 HTTP 上下文",
        ))
    })?
    .matched)
}

/// Evaluates one HTTP condition at the actor boundary.
pub fn evaluate_http_condition(
    condition: &Condition,
    http_matches: &mut impl FnMut(&MatchField, &MatchOperator) -> Result<bool, String>,
) -> Result<ConditionEvaluation, String> {
    let evaluated = match condition {
        Condition::Http { field, operator } => {
            ConditionEvaluation::ordinary(http_matches(field, operator)?)
        }
        Condition::Document { .. } | Condition::DocumentPattern { .. } => {
            return Err("Document 条件必须由统一 Document program 负责".into());
        }
    };
    Ok(evaluated)
}

/// Evaluates one HTTP condition against the authoritative typed match context.
pub fn evaluate_http_context_condition(
    condition: &Condition,
    context: &MatchContext<'_>,
) -> Result<ConditionEvaluation, DomainError> {
    evaluate_http_condition(condition, &mut |field, operator| {
        matches_http_condition(field, operator, context).map_err(|error| error.message)
    })
    .map_err(|message| {
        DomainError::new(ErrorCode::RuleInvalid, "统一 HTTP 条件运行时匹配失败")
            .with_field_error("condition", message)
    })
}

/// Validates declared Schema paths while preserving undeclared paths as rule-local metadata.
pub fn validate_document_condition_schema(
    condition: &Condition,
    schema: &DocumentSchemaNode,
) -> Result<(), DomainError> {
    match condition {
        Condition::Document { path, predicate } => {
            if let Ok(node) = schema.resolve(path)
                && !node.accepts(predicate.value_type())
            {
                return Err(rule_error(path.as_str(), "条件值类型与 Schema 声明不一致"));
            }
        }
        Condition::DocumentPattern { path, predicate } => {
            let nodes = schema.resolve_match_path(path);
            if !nodes.is_empty()
                && !nodes
                    .iter()
                    .any(|node| node.accepts(predicate.value_type()))
            {
                return Err(rule_error(path.as_str(), "条件值类型与 Schema 声明不一致"));
            }
        }
        Condition::Http { .. } => {}
    }
    Ok(())
}

/// Returns the independently owned path/type metadata carried by Document conditions.
#[must_use]
pub fn document_condition_path_types(
    condition: &Condition,
) -> BTreeMap<JsonPointer, DocumentValueType> {
    match condition {
        Condition::Document { path, predicate } => {
            BTreeMap::from([(path.clone(), predicate.value_type())])
        }
        Condition::DocumentPattern { .. } | Condition::Http { .. } => BTreeMap::new(),
    }
}

/// Strict Document mutation primitives shared by HTTP and Socket rules.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentMutation {
    /// Strict Set using [`Document::set`].
    Set {
        path: JsonPointer,
        value: DocumentValue,
    },
    /// Strict Clear using [`Document::clear_path`].
    Clear {
        path: JsonPointer,
        value_type: DocumentValueType,
    },
    /// Strict array Insert using [`Document::insert`].
    Insert {
        path: JsonPointer,
        index: usize,
        value: DocumentValue,
    },
    /// Strict array Append using [`Document::append`].
    Append {
        path: JsonPointer,
        value: DocumentValue,
    },
}

/// The one action paired with a rule condition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "source", content = "value", rename_all = "snake_case")]
pub enum UnifiedAction {
    /// Records a match without mutating protocol data.
    RecordMatch,
    /// Document mutation.
    Document(DocumentMutation),
    /// Existing typed non-terminal HTTP/runtime action.
    Http(HttpAction),
    /// Terminal effect that stops later rules.
    Terminal(TerminalAction),
}

fn rule_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "统一规则配置无效").with_field_error(field, message)
}
