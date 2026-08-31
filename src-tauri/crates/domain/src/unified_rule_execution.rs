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
    /// A typed Document predicate whose condition-only path may contain `*` segments.
    DocumentPattern {
        /// Condition-only match path.
        path: DocumentMatchPath,
        /// Strict predicate applied with ANY semantics to expanded values.
        predicate: DocumentPredicate,
    },
    /// Existing typed HTTP/runtime condition, retained as a leaf rather than a parallel tree.
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
                Self::Leaf(Condition::Document { .. } | Condition::DocumentPattern { .. }) => {
                    return true;
                }
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
        self.matches_with(document, 1, &mut |_, _| {
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
        http_matches: &mut impl FnMut(&MatchField, &MatchOperator) -> Result<bool, E>,
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
        http_matches: &mut impl FnMut(&MatchField, &MatchOperator) -> Result<bool, E>,
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
            Self::Leaf(Condition::DocumentPattern { path, predicate }) => {
                Ok(ConditionEvaluation::ordinary(
                    document
                        .resolve_match_path(path)
                        .into_iter()
                        .any(|actual| predicate.matches(actual)),
                ))
            }
            Self::Leaf(Condition::Http { field, operator }) => {
                http_matches(field, operator).map(ConditionEvaluation::ordinary)
            }
            Self::Leaf(Condition::NthHit { count }) => Ok(ConditionEvaluation {
                matched: *count > 0 && nth_attempt == *count,
                eligible_without_nth: true,
                contains_nth: true,
            }),
        }
    }

    /// Evaluates the authoritative recursive tree at the HTTP-only actor boundary.
    ///
    /// Document leaves belong to the joint Document program and are rejected when no joint
    /// program owns the rule. This keeps ordinary HTTP execution on the same tree without a
    /// legacy flattened representation.
    pub fn evaluate_http_with_nth(
        &self,
        nth_attempt: u64,
        http_matches: &mut impl FnMut(&MatchField, &MatchOperator) -> Result<bool, String>,
    ) -> Result<ConditionEvaluation, String> {
        match self {
            Self::All(children) => {
                let mut result = ConditionEvaluation {
                    matched: true,
                    eligible_without_nth: true,
                    contains_nth: false,
                };
                for child in children {
                    let child = child.evaluate_http_with_nth(nth_attempt, http_matches)?;
                    result.matched &= child.matched;
                    result.eligible_without_nth &= child.eligible_without_nth;
                    result.contains_nth |= child.contains_nth;
                }
                Ok(result)
            }
            Self::Any(children) => {
                let mut result = ConditionEvaluation {
                    matched: false,
                    eligible_without_nth: false,
                    contains_nth: false,
                };
                for child in children {
                    let child = child.evaluate_http_with_nth(nth_attempt, http_matches)?;
                    result.matched |= child.matched;
                    result.eligible_without_nth |= child.eligible_without_nth;
                    result.contains_nth |= child.contains_nth;
                }
                Ok(result)
            }
            Self::Leaf(Condition::Http { field, operator }) => {
                http_matches(field, operator).map(ConditionEvaluation::ordinary)
            }
            Self::Leaf(Condition::NthHit { count }) => Ok(ConditionEvaluation {
                matched: *count > 0 && nth_attempt == *count,
                eligible_without_nth: true,
                contains_nth: true,
            }),
            Self::Leaf(Condition::Document { .. } | Condition::DocumentPattern { .. }) => {
                Err("Document 条件必须由统一 Document program 负责".into())
            }
        }
    }

    /// Evaluates an HTTP-only tree against the authoritative typed match context.
    pub fn evaluate_http_context_with_nth(
        &self,
        nth_attempt: u64,
        context: &MatchContext<'_>,
    ) -> Result<ConditionEvaluation, DomainError> {
        self.evaluate_http_with_nth(nth_attempt, &mut |field, operator| {
            matches_http_condition(field, operator, context).map_err(|error| error.message)
        })
        .map_err(|message| {
            DomainError::new(ErrorCode::RuleInvalid, "统一 HTTP 条件树运行时匹配失败")
                .with_field_error("condition", message)
        })
    }

    /// Validates declared Schema paths while preserving undeclared paths as rule-local metadata.
    pub fn validate_document_schema(&self, schema: &DocumentSchemaNode) -> Result<(), DomainError> {
        match self {
            Self::All(children) | Self::Any(children) => {
                for child in children {
                    child.validate_document_schema(schema)?;
                }
                Ok(())
            }
            Self::Leaf(Condition::Document { path, predicate }) => {
                if let Ok(node) = schema.resolve(path)
                    && !node.accepts(predicate.value_type())
                {
                    return Err(rule_error(path.as_str(), "条件值类型与 Schema 声明不一致"));
                }
                Ok(())
            }
            Self::Leaf(Condition::DocumentPattern { path, predicate }) => {
                let nodes = schema.resolve_match_path(path);
                if !nodes.is_empty()
                    && !nodes
                        .iter()
                        .any(|node| node.accepts(predicate.value_type()))
                {
                    return Err(rule_error(path.as_str(), "条件值类型与 Schema 声明不一致"));
                }
                Ok(())
            }
            Self::Leaf(Condition::Http { .. } | Condition::NthHit { .. }) => Ok(()),
        }
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
            Self::Leaf(
                Condition::DocumentPattern { .. }
                | Condition::Http { .. }
                | Condition::NthHit { .. },
            ) => Ok(()),
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
