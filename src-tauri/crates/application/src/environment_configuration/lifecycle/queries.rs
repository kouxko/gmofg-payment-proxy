use super::{
    registry::EnvironmentCandidateRegistry,
    state::CandidateEntry,
    types::{
        EnvironmentCandidateId, EnvironmentCandidateMetrics, EnvironmentCandidateStatus,
        EnvironmentCandidateStatusResult,
    },
};

impl EnvironmentCandidateRegistry {
    pub fn status(
        &self,
        candidate_id: &EnvironmentCandidateId,
    ) -> EnvironmentCandidateStatusResult {
        self.shared
            .state
            .lock()
            .candidates
            .get(candidate_id)
            .map_or_else(
                || EnvironmentCandidateStatusResult::not_found(candidate_id.clone()),
                CandidateEntry::public_status,
            )
    }

    pub fn metrics(&self) -> EnvironmentCandidateMetrics {
        let state = self.shared.state.lock();
        EnvironmentCandidateMetrics {
            active_candidates: state
                .candidates
                .values()
                .filter(|entry| entry.status.is_active())
                .count(),
            active_apply_tasks: state
                .candidates
                .values()
                .filter(|entry| entry.status.is_apply_active())
                .count(),
            private_candidate_bytes: state
                .candidates
                .values()
                .map(|entry| entry.private_bytes)
                .sum(),
            retained_terminal_candidates: state.terminal_order.len(),
            terminal_public_bytes: state.terminal_public_bytes,
            shutdown_draining_apply_tasks: usize::from(state.shutting_down)
                * state
                    .candidates
                    .values()
                    .filter(|entry| entry.status == EnvironmentCandidateStatus::ApplyInProgress)
                    .count(),
        }
    }
}
