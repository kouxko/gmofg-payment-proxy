//! Phase 5 unified rule condition, action, validation, and deterministic execution primitives.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Document, DocumentAction, DocumentCondition, DocumentSchemaNode, DocumentValue,
    DocumentValueType, DomainError, ErrorCode, JsonPointer, MatchCondition, RuleAction, RuleId,
    TerminalAction,
};

mod condition_evaluation;
mod program;

pub use condition_evaluation::ConditionEvaluation;
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

/// One typed leaf in a unified condition tree.
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
    /// Existing typed HTTP/runtime condition, retained as a leaf rather than a parallel tree.
    Http {
        /// Typed HTTP condition.
        condition: MatchCondition,
    },
    /// Shared lifecycle predicate, independent from HTTP capabilities.
    NthHit {
        /// Exact next successful hit number.
        count: u64,
    },
}

impl From<MatchCondition> for Condition {
    fn from(condition: MatchCondition) -> Self {
        Self::Http { condition }
    }
}

/// Recursive non-empty AND/OR condition tree. NOT is intentionally not representable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "operator", content = "children", rename_all = "snake_case")]
pub enum ConditionTree {
    /// All child nodes must match.
    All(Vec<ConditionTree>),
    /// At least one child node must match.
    Any(Vec<ConditionTree>),
    /// Typed leaf condition.
    Leaf(Condition),
}

impl ConditionTree {
    /// Counts typed leaf predicates for summaries and diagnostics.
    #[must_use]
    pub fn leaf_count(&self) -> usize {
        match self {
            Self::All(children) | Self::Any(children) => {
                children.iter().map(Self::leaf_count).sum()
            }
            Self::Leaf(_) => 1,
        }
    }
    /// Builds one AND tree from the legacy HTTP runtime projection.
    #[must_use]
    pub fn from_http_conditions(conditions: impl IntoIterator<Item = MatchCondition>) -> Self {
        Self::All(
            conditions
                .into_iter()
                .map(|condition| Self::Leaf(Condition::Http { condition }))
                .collect(),
        )
    }

    /// Builds one AND tree from the Phase-12 legacy Document runtime projection.
    #[must_use]
    pub fn from_document_conditions(
        conditions: impl IntoIterator<Item = DocumentCondition>,
    ) -> Self {
        Self::All(
            conditions
                .into_iter()
                .map(|condition| match condition {
                    DocumentCondition::Equals { field, value } => Self::Leaf(Condition::Document {
                        path: field,
                        predicate: match value {
                            DocumentValue::String(value) => {
                                DocumentPredicate::String(StringPredicate {
                                    operator: StringOperator::Equal,
                                    value,
                                })
                            }
                            DocumentValue::Number(value) => {
                                DocumentPredicate::Number(NumberPredicate {
                                    operator: NumberOperator::Equal,
                                    value,
                                })
                            }
                            DocumentValue::Boolean(value) => {
                                DocumentPredicate::Boolean(BooleanPredicate::Equal(value))
                            }
                            DocumentValue::Null(()) => DocumentPredicate::NullEqual,
                            DocumentValue::Object(_) | DocumentValue::Array(_) => {
                                return Self::All(Vec::new());
                            }
                        },
                    }),
                })
                .collect(),
        )
    }
    /// Creates a non-empty AND group.
    pub fn all(children: Vec<Self>) -> Result<Self, DomainError> {
        let tree = Self::All(children);
        tree.validate()?;
        Ok(tree)
    }

    /// Creates a non-empty OR group.
    pub fn any(children: Vec<Self>) -> Result<Self, DomainError> {
        let tree = Self::Any(children);
        tree.validate()?;
        Ok(tree)
    }

    /// Validates that every AND/OR group is non-empty.
    pub fn validate(&self) -> Result<(), DomainError> {
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match node {
                Self::All(children) | Self::Any(children) => {
                    if children.is_empty() {
                        return Err(rule_error("condition.children", "AND/OR 条件组不能为空"));
                    }
                    pending.extend(children);
                }
                Self::Leaf(Condition::NthHit { count: 0 }) => {
                    return Err(rule_error("condition.count", "第 N 次命中的次数必须大于 0"));
                }
                Self::Leaf(_) => {}
            }
        }
        Ok(())
    }

    pub(crate) fn contains_document_condition(&self) -> bool {
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match node {
                Self::All(children) | Self::Any(children) => pending.extend(children),
                Self::Leaf(Condition::Document { .. }) => return true,
                Self::Leaf(Condition::Http { .. } | Condition::NthHit { .. }) => {}
            }
        }
        false
    }

    /// Matches only the Document leaves in this tree.
    ///
    /// A typed HTTP leaf requires the Application-owned HTTP context evaluator and is rejected at
    /// this Document-only boundary. A missing path or runtime JSON type mismatch is a normal false.
    pub fn matches_document(&self, document: &Document) -> Result<bool, DomainError> {
        self.matches_with(document, 1, &mut |_| {
            Err(rule_error(
                "condition",
                "HTTP 条件需要应用层提供类型化 HTTP 上下文",
            ))
        })
    }

    /// Matches this tree using a caller-provided evaluator for typed HTTP leaves.
    pub fn matches_with<E>(
        &self,
        document: &Document,
        nth_attempt: u64,
        http_matches: &mut impl FnMut(&MatchCondition) -> Result<bool, E>,
    ) -> Result<bool, E>
    where
        E: From<DomainError>,
    {
        Ok(self
            .evaluate_with_nth(document, nth_attempt, http_matches)?
            .matched)
    }

    pub fn evaluate_with_nth<E>(
        &self,
        document: &Document,
        nth_attempt: u64,
        http_matches: &mut impl FnMut(&MatchCondition) -> Result<bool, E>,
    ) -> Result<ConditionEvaluation, E>
    where
        E: From<DomainError>,
    {
        match self {
            Self::All(children) => {
                let mut matched = true;
                let mut eligible_without_nth = true;
                let mut contains_nth = false;
                for child in children {
                    let evaluated = child.evaluate_with_nth(document, nth_attempt, http_matches)?;
                    matched &= evaluated.matched;
                    eligible_without_nth &= evaluated.eligible_without_nth;
                    contains_nth |= evaluated.contains_nth;
                }
                Ok(ConditionEvaluation {
                    matched,
                    eligible_without_nth,
                    contains_nth,
                })
            }
            Self::Any(children) => {
                let mut matched = false;
                let mut eligible_without_nth = false;
                let mut contains_nth = false;
                for child in children {
                    let evaluated = child.evaluate_with_nth(document, nth_attempt, http_matches)?;
                    matched |= evaluated.matched;
                    eligible_without_nth |= evaluated.eligible_without_nth;
                    contains_nth |= evaluated.contains_nth;
                }
                Ok(ConditionEvaluation {
                    matched,
                    eligible_without_nth,
                    contains_nth,
                })
            }
            Self::Leaf(Condition::Document { path, predicate }) => match document.resolve(path) {
                Ok(actual) => Ok(ConditionEvaluation::ordinary(predicate.matches(actual))),
                Err(DomainError {
                    code: ErrorCode::DocumentPathMissing | ErrorCode::DocumentPathTypeMismatch,
                    ..
                }) => Ok(ConditionEvaluation::ordinary(false)),
                Err(error) => Err(error.into()),
            },
            Self::Leaf(Condition::Http { condition }) => {
                http_matches(condition).map(ConditionEvaluation::ordinary)
            }
            Self::Leaf(Condition::NthHit { count }) => Ok(ConditionEvaluation {
                matched: *count > 0 && nth_attempt == *count,
                eligible_without_nth: true,
                contains_nth: true,
            }),
        }
    }

    /// Validates declared Schema paths while preserving undeclared paths as rule-local metadata.
    pub fn validate_document_schema(&self, schema: &DocumentSchemaNode) -> Result<(), DomainError> {
        self.visit_document_leaves(&mut |path, predicate| {
            if let Ok(node) = schema.resolve(path)
                && !node.accepts(predicate.value_type())
            {
                return Err(rule_error(path.as_str(), "条件值类型与 Schema 声明不一致"));
            }
            Ok(())
        })
    }

    /// Returns the independently owned path/type metadata carried by Document leaves.
    #[must_use]
    pub fn document_path_types(&self) -> BTreeMap<JsonPointer, DocumentValueType> {
        let mut result = BTreeMap::new();
        let _ = self.visit_document_leaves(&mut |path, predicate| {
            result.insert(path.clone(), predicate.value_type());
            Ok(())
        });
        result
    }

    fn visit_document_leaves(
        &self,
        visitor: &mut impl FnMut(&JsonPointer, &DocumentPredicate) -> Result<(), DomainError>,
    ) -> Result<(), DomainError> {
        match self {
            Self::All(children) | Self::Any(children) => {
                for child in children {
                    child.visit_document_leaves(visitor)?;
                }
                Ok(())
            }
            Self::Leaf(Condition::Document { path, predicate }) => visitor(path, predicate),
            Self::Leaf(Condition::Http { .. } | Condition::NthHit { .. }) => Ok(()),
        }
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
    Clear { path: JsonPointer },
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

impl DocumentMutation {
    pub fn apply(&self, document: &mut Document) -> Result<(), DomainError> {
        match self {
            Self::Set { path, value } => document.set(path, value.clone()),
            Self::Clear { path } => document.clear_path(path),
            Self::Insert { path, index, value } => document.insert(path, *index, value.clone()),
            Self::Append { path, value } => document.append(path, value.clone()),
        }
    }
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
    Http(RuleAction),
    /// Terminal effect; at most one is allowed and it must be last.
    Terminal(TerminalAction),
}

/// Validates Document mutation values at paths declared by the package Schema.
pub fn validate_unified_actions_schema(
    actions: &[UnifiedAction],
    schema: &DocumentSchemaNode,
) -> Result<(), DomainError> {
    for (index, action) in actions.iter().enumerate() {
        let expected_and_value = match action {
            UnifiedAction::Document(DocumentMutation::Set { path, value }) => {
                schema.resolve(path).ok().map(|node| (node, value))
            }
            UnifiedAction::Document(
                DocumentMutation::Insert { path, value, .. }
                | DocumentMutation::Append { path, value },
            ) => match schema.resolve(path).ok() {
                Some(DocumentSchemaNode::Array { items, .. }) => Some((items.as_ref(), value)),
                _ => None,
            },
            UnifiedAction::RecordMatch
            | UnifiedAction::Document(DocumentMutation::Clear { .. })
            | UnifiedAction::Http(_)
            | UnifiedAction::Terminal(_) => None,
        };
        if let Some((expected, value)) = expected_and_value
            && !expected.accepts(value.value_type())
        {
            return Err(rule_error(
                &format!("actions.{index}"),
                "动作值类型与 Schema 声明不一致",
            ));
        }
    }
    Ok(())
}

impl From<RuleAction> for UnifiedAction {
    fn from(action: RuleAction) -> Self {
        match action {
            RuleAction::Terminal(action) => Self::Terminal(action),
            action => Self::Http(action),
        }
    }
}

impl From<DocumentAction> for UnifiedAction {
    fn from(action: DocumentAction) -> Self {
        Self::Document(match action {
            DocumentAction::SetField { field, value } => {
                DocumentMutation::Set { path: field, value }
            }
            DocumentAction::ClearField { field } => DocumentMutation::Clear { path: field },
            DocumentAction::RecordMatch => return Self::RecordMatch,
        })
    }
}

fn rule_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "统一规则配置无效").with_field_error(field, message)
}
