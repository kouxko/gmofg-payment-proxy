/// Result of one condition pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConditionEvaluation {
    pub matched: bool,
}

/// Declares whether one rule's condition tree is owned by the unified Document runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleConditionEvaluation {
    UnifiedOwned(ConditionEvaluation),
    NotOwned,
}

impl ConditionEvaluation {
    pub(super) const fn ordinary(matched: bool) -> Self {
        Self { matched }
    }
}
use crate::{DocumentValue, DocumentValueType};

use super::{BooleanPredicate, DocumentPredicate, NumberOperator, StringOperator};

impl DocumentPredicate {
    #[must_use]
    pub const fn value_type(&self) -> DocumentValueType {
        match self {
            Self::String(_) => DocumentValueType::String,
            Self::Number(_) => DocumentValueType::Number,
            Self::Boolean(_) => DocumentValueType::Boolean,
            Self::NullEqual => DocumentValueType::Null,
        }
    }

    pub(super) fn matches(&self, actual: &DocumentValue) -> bool {
        match (self, actual) {
            (Self::String(expected), DocumentValue::String(actual)) => match expected.operator {
                StringOperator::Equal => actual == &expected.value,
                StringOperator::Contains => actual.contains(&expected.value),
                StringOperator::StartsWith => actual.starts_with(&expected.value),
                StringOperator::EndsWith => actual.ends_with(&expected.value),
            },
            (Self::Number(expected), DocumentValue::Number(actual)) => {
                let actual = actual.get();
                let expected_value = expected.value.get();
                match expected.operator {
                    NumberOperator::Equal => matches!(
                        actual.partial_cmp(&expected_value),
                        Some(std::cmp::Ordering::Equal)
                    ),
                    NumberOperator::Less => actual < expected_value,
                    NumberOperator::LessEqual => actual <= expected_value,
                    NumberOperator::Greater => actual > expected_value,
                    NumberOperator::GreaterEqual => actual >= expected_value,
                }
            }
            (Self::Boolean(BooleanPredicate::Equal(expected)), DocumentValue::Boolean(actual)) => {
                actual == expected
            }
            (Self::NullEqual, DocumentValue::Null(())) => true,
            _ => false,
        }
    }
}
