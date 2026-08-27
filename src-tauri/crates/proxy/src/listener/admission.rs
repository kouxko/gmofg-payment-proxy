use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

use crate::{ErrorCode, ProxyError, Result};

#[derive(Debug)]
pub(crate) enum AdmissionDecision {
    Admitted(OwnedSemaphorePermit),
    CapacityExhausted,
}

#[derive(Debug, Clone)]
pub(crate) struct ListenerAdmission {
    capacity: ListenerCapacity,
}

#[derive(Debug, Clone)]
pub(crate) struct ListenerCapacity {
    permits: Arc<Semaphore>,
}

impl ListenerCapacity {
    pub(crate) fn new(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "listener connection capacity must be greater than zero",
            ));
        }
        Ok(Self {
            permits: Arc::new(Semaphore::new(capacity)),
        })
    }

    fn try_acquire(&self) -> std::result::Result<OwnedSemaphorePermit, TryAcquireError> {
        Arc::clone(&self.permits).try_acquire_owned()
    }
}

impl ListenerAdmission {
    pub(crate) const fn new(capacity: ListenerCapacity) -> Self {
        Self { capacity }
    }

    pub(crate) fn admit(&self) -> AdmissionDecision {
        match self.capacity.try_acquire() {
            Ok(permit) => AdmissionDecision::Admitted(permit),
            Err(TryAcquireError::NoPermits | TryAcquireError::Closed) => {
                AdmissionDecision::CapacityExhausted
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_is_enforced() {
        let admission = ListenerAdmission::new(ListenerCapacity::new(1).unwrap());
        let _permit = match admission.admit() {
            AdmissionDecision::Admitted(permit) => permit,
            decision @ AdmissionDecision::CapacityExhausted => {
                panic!("expected admitted, got {decision:?}")
            }
        };
        assert!(matches!(
            admission.admit(),
            AdmissionDecision::CapacityExhausted
        ));
    }
}
