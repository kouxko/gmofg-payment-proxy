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
pub use mutation::validate_unified_actions_schema;
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

/// One typed condition in a flat rule condition list.
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
    /// Shared lifecycle predicate, independent from HTTP capabilities.
    NthHit {
        /// Exact next successful hit number.
        count: u64,
    },
}

/// Validates the non-empty flat condition list. Every condition is combined with AND.
pub fn validate_conditions(conditions: &[Condition]) -> Result<(), DomainError> {
    if conditions.is_empty() {
        return Err(rule_error("conditions", "条件列表不能为空"));
    }
    if conditions
        .iter()
        .any(|condition| matches!(condition, Condition::NthHit { count: 0 }))
    {
        return Err(rule_error(
            "conditions.count",
            "第 N 次命中的次数必须大于 0",
        ));
    }
    Ok(())
}

#[must_use]
pub fn contains_document_condition(conditions: &[Condition]) -> bool {
    conditions.iter().any(|condition| {
        matches!(
            condition,
            Condition::Document { .. } | Condition::DocumentPattern { .. }
        )
    })
}

/// Evaluates a flat AND list using a caller-provided evaluator for typed HTTP conditions.
pub fn evaluate_conditions_with_nth<E>(
    conditions: &[Condition],
    document: &Document,
    nth_attempt: u64,
    http_matches: &mut impl FnMut(&MatchField, &MatchOperator) -> Result<bool, E>,
) -> Result<ConditionEvaluation, E>
where
    E: From<DomainError>,
{
    validate_conditions(conditions)?;
    let mut result = ConditionEvaluation {
        matched: true,
        eligible_without_nth: true,
        contains_nth: false,
    };
    for condition in conditions {
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
            Condition::NthHit { count } => ConditionEvaluation {
                matched: nth_attempt == *count,
                eligible_without_nth: true,
                contains_nth: true,
            },
        };
        result.matched &= evaluated.matched;
        result.eligible_without_nth &= evaluated.eligible_without_nth;
        result.contains_nth |= evaluated.contains_nth;
    }
    Ok(result)
}

/// Matches only Document conditions. HTTP conditions are rejected at this boundary.
pub fn matches_document_conditions(
    conditions: &[Condition],
    document: &Document,
) -> Result<bool, DomainError> {
    Ok(
        evaluate_conditions_with_nth(conditions, document, 1, &mut |_, _| {
            Err(rule_error(
                "conditions",
                "HTTP 条件需要应用层提供类型化 HTTP 上下文",
            ))
        })?
        .matched,
    )
}

/// Evaluates HTTP-only flat conditions at the actor boundary.
pub fn evaluate_http_conditions_with_nth(
    conditions: &[Condition],
    nth_attempt: u64,
    http_matches: &mut impl FnMut(&MatchField, &MatchOperator) -> Result<bool, String>,
) -> Result<ConditionEvaluation, String> {
    validate_conditions(conditions).map_err(|error| error.message)?;
    let mut result = ConditionEvaluation {
        matched: true,
        eligible_without_nth: true,
        contains_nth: false,
    };
    for condition in conditions {
        let evaluated = match condition {
            Condition::Http { field, operator } => {
                ConditionEvaluation::ordinary(http_matches(field, operator)?)
            }
            Condition::NthHit { count } => ConditionEvaluation {
                matched: nth_attempt == *count,
                eligible_without_nth: true,
                contains_nth: true,
            },
            Condition::Document { .. } | Condition::DocumentPattern { .. } => {
                return Err("Document 条件必须由统一 Document program 负责".into());
            }
        };
        result.matched &= evaluated.matched;
        result.eligible_without_nth &= evaluated.eligible_without_nth;
        result.contains_nth |= evaluated.contains_nth;
    }
    Ok(result)
}

/// Evaluates HTTP-only flat conditions against the authoritative typed match context.
pub fn evaluate_http_context_conditions_with_nth(
    conditions: &[Condition],
    nth_attempt: u64,
    context: &MatchContext<'_>,
) -> Result<ConditionEvaluation, DomainError> {
    evaluate_http_conditions_with_nth(conditions, nth_attempt, &mut |field, operator| {
        matches_http_condition(field, operator, context).map_err(|error| error.message)
    })
    .map_err(|message| {
        DomainError::new(ErrorCode::RuleInvalid, "统一 HTTP 条件运行时匹配失败")
            .with_field_error("conditions", message)
    })
}

/// Validates declared Schema paths while preserving undeclared paths as rule-local metadata.
pub fn validate_document_conditions_schema(
    conditions: &[Condition],
    schema: &DocumentSchemaNode,
) -> Result<(), DomainError> {
    validate_conditions(conditions)?;
    for condition in conditions {
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
            Condition::Http { .. } | Condition::NthHit { .. } => {}
        }
    }
    Ok(())
}

/// Returns the independently owned path/type metadata carried by Document conditions.
#[must_use]
pub fn document_condition_path_types(
    conditions: &[Condition],
) -> BTreeMap<JsonPointer, DocumentValueType> {
    conditions
        .iter()
        .filter_map(|condition| match condition {
            Condition::Document { path, predicate } => Some((path.clone(), predicate.value_type())),
            Condition::DocumentPattern { .. }
            | Condition::Http { .. }
            | Condition::NthHit { .. } => None,
        })
        .collect()
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

/// One item in the single ordered action list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "source", content = "value", rename_all = "snake_case")]
pub enum UnifiedAction {
    /// Records a match without mutating protocol data.
    RecordMatch,
    /// Document mutation.
    Document(DocumentMutation),
    /// Existing typed non-terminal HTTP/runtime action.
    Http(HttpAction),
    /// Terminal effect; at most one is allowed and it must be last.
    Terminal(TerminalAction),
}

fn rule_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "统一规则配置无效").with_field_error(field, message)
}
