use super::{EnvironmentApplyGenerations, EnvironmentApplyLease, EnvironmentApplyLeaseOutcome};

impl EnvironmentApplyLease {
    pub fn observed(&self) -> &EnvironmentApplyGenerations {
        &self.observed
    }

    pub const fn outcome(&self) -> EnvironmentApplyLeaseOutcome {
        self.outcome
    }

    pub const fn is_package_stale(&self) -> bool {
        matches!(self.outcome, EnvironmentApplyLeaseOutcome::PackageStale)
    }

    pub const fn is_generation_mismatch(&self) -> bool {
        matches!(
            self.outcome,
            EnvironmentApplyLeaseOutcome::GenerationMismatch
        )
    }
}

impl Drop for EnvironmentApplyLease {
    fn drop(&mut self) {
        if let Some(release_reverse_order) = self.release_reverse_order.take() {
            release_reverse_order();
        }
    }
}

impl std::fmt::Debug for EnvironmentApplyLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentApplyLease")
            .field("observed", &self.observed)
            .field("outcome", &self.outcome)
            .finish_non_exhaustive()
    }
}
