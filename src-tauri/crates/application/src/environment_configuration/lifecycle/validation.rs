use std::{collections::HashMap, sync::OnceLock};

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::{
    registry::EnvironmentCandidateRegistry,
    types::{EnvironmentCandidateId, EnvironmentCandidateLifecycleError},
};
use crate::environment_configuration::EnvironmentValidatedApplyBaseline;
use crate::environment_configuration::{EnvironmentCandidateStatus, EnvironmentStatusCode};
use crate::environment_configuration::{
    EnvironmentDiagnostic, EnvironmentTerminalResult, EnvironmentValidationLayerResult,
    EnvironmentValidationReport,
};

static CANCELLATION_CODES: OnceLock<Mutex<HashMap<CancellationToken, EnvironmentStatusCode>>> =
    OnceLock::new();

pub(super) fn signal_validation_cancellation(
    token: &CancellationToken,
    code: EnvironmentStatusCode,
) {
    CANCELLATION_CODES
        .get_or_init(Default::default)
        .lock()
        .insert(token.clone(), code);
    token.cancel();
}

pub(crate) fn take_validation_cancellation_code(
    token: &CancellationToken,
) -> Option<EnvironmentStatusCode> {
    CANCELLATION_CODES
        .get_or_init(Default::default)
        .lock()
        .remove(token)
}

pub(super) fn discard_validation_cancellation_code(token: &CancellationToken) {
    CANCELLATION_CODES
        .get_or_init(Default::default)
        .lock()
        .remove(token);
}

impl EnvironmentCandidateRegistry {
    pub(crate) fn attach_validated_apply_baseline(
        &self,
        candidate_id: &EnvironmentCandidateId,
        baseline: EnvironmentValidatedApplyBaseline,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        let mut state = self.shared.state.lock();
        let entry = state
            .candidates
            .get_mut(candidate_id)
            .ok_or(EnvironmentCandidateLifecycleError::CandidateNotFound)?;
        if entry.status != EnvironmentCandidateStatus::Validating
            || entry.validated_apply_baseline.is_some()
        {
            return Err(EnvironmentCandidateLifecycleError::InvalidState);
        }
        entry.validated_apply_baseline = Some(baseline);
        Ok(())
    }

    pub(crate) fn validation_cancellation(
        &self,
        candidate_id: &EnvironmentCandidateId,
    ) -> Result<CancellationToken, EnvironmentCandidateLifecycleError> {
        let state = self.shared.state.lock();
        let entry = state
            .candidates
            .get(candidate_id)
            .ok_or(EnvironmentCandidateLifecycleError::CandidateNotFound)?;
        if entry.status != EnvironmentCandidateStatus::Validating {
            return Err(EnvironmentCandidateLifecycleError::InvalidState);
        }
        Ok(entry.validation_cancellation.clone())
    }

    pub(crate) fn complete_validation_report_failed(
        &self,
        candidate_id: &EnvironmentCandidateId,
        report: EnvironmentValidationReport,
    ) -> Result<(), EnvironmentCandidateLifecycleError> {
        let status_code = report
            .status_code()
            .ok_or(EnvironmentCandidateLifecycleError::InvalidState)?;
        let validation_layers = report
            .into_layers()
            .into_iter()
            .map(|result| EnvironmentValidationLayerResult::from_orchestrated(&result))
            .collect();
        let mut state = self.shared.state.lock();
        let entry = state
            .candidates
            .get(candidate_id)
            .ok_or(EnvironmentCandidateLifecycleError::CandidateNotFound)?;
        if entry.status != EnvironmentCandidateStatus::Validating {
            return if matches!(
                entry.status,
                EnvironmentCandidateStatus::Cancelled
                    | EnvironmentCandidateStatus::CancelledByShutdown
            ) {
                Ok(())
            } else {
                Err(EnvironmentCandidateLifecycleError::InvalidState)
            };
        }
        self.shared.publish_terminal(
            &mut state,
            candidate_id,
            EnvironmentCandidateStatus::ValidationFailed,
            EnvironmentTerminalResult::validation_failed(status_code),
            Some(validation_layers),
            Some(vec![EnvironmentDiagnostic::error(status_code)]),
        )
    }
}
