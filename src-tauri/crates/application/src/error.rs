use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

use crate::RuntimeEpoch;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppErrorViewModel {
    pub code: String,
    pub message: String,
    pub field_errors: BTreeMap<String, Vec<String>>,
    pub retryable: bool,
    pub suggested_action: Option<String>,
    pub entity_id: Option<String>,
    pub runtime_epoch: Option<RuntimeEpoch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{view_model:?}")]
pub struct AppError {
    pub view_model: Box<AppErrorViewModel>,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            view_model: Box::new(AppErrorViewModel {
                code: code.into(),
                message: message.into(),
                field_errors: BTreeMap::new(),
                retryable: false,
                suggested_action: None,
                entity_id: None,
                runtime_epoch: None,
            }),
        }
    }

    pub fn field(
        code: impl Into<String>,
        message: impl Into<String>,
        field_errors: BTreeMap<String, Vec<String>>,
    ) -> Self {
        let mut error = Self::new(code, message);
        error.view_model.field_errors = field_errors;
        error
    }

    #[must_use]
    pub fn retryable(mut self, suggested_action: impl Into<String>) -> Self {
        self.view_model.retryable = true;
        self.view_model.suggested_action = Some(suggested_action.into());
        self
    }

    #[must_use]
    pub fn entity(mut self, entity_id: impl Into<String>) -> Self {
        self.view_model.entity_id = Some(entity_id.into());
        self
    }

    #[must_use]
    pub fn epoch(mut self, runtime_epoch: RuntimeEpoch) -> Self {
        self.view_model.runtime_epoch = Some(runtime_epoch);
        self
    }
}

impl From<AppError> for AppErrorViewModel {
    fn from(value: AppError) -> Self {
        *value.view_model
    }
}

impl From<gmofg_proxy_domain::DomainError> for AppError {
    fn from(value: gmofg_proxy_domain::DomainError) -> Self {
        Self {
            view_model: Box::new(AppErrorViewModel {
                code: value.code.as_str().to_owned(),
                message: value.message,
                field_errors: *value.field_errors,
                retryable: value.retryable,
                suggested_action: value.suggested_action,
                entity_id: value.entity_id,
                runtime_epoch: value
                    .runtime_epoch
                    .map(gmofg_proxy_domain::RuntimeEpoch::as_uuid),
            }),
        }
    }
}
