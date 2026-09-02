use std::sync::Arc;

use crate::{
    ExchangeObservationPage, ExchangeObservationQuery, ExchangeObservationQueryPort,
    ExchangeObservationRecord,
};

/// Application-facing read API for the shared bounded Exchange observation store.
#[derive(Clone)]
pub struct ExchangeObservationQueries {
    port: Arc<dyn ExchangeObservationQueryPort>,
}

impl ExchangeObservationQueries {
    #[must_use]
    pub fn new(port: Arc<dyn ExchangeObservationQueryPort>) -> Self {
        Self { port }
    }

    #[must_use]
    pub fn query(&self, query: &ExchangeObservationQuery) -> ExchangeObservationPage {
        self.port.query(query)
    }

    #[must_use]
    pub fn get(&self, exchange_id: &str) -> Option<ExchangeObservationRecord> {
        self.port.get(exchange_id)
    }
}

impl<P> From<Arc<P>> for ExchangeObservationQueries
where
    P: ExchangeObservationQueryPort + 'static,
{
    fn from(port: Arc<P>) -> Self {
        Self::new(port)
    }
}

impl std::fmt::Debug for ExchangeObservationQueries {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExchangeObservationQueries")
            .finish_non_exhaustive()
    }
}
