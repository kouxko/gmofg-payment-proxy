use tokio::task::AbortHandle;

/// Aborts a connection actor unless registration transferred ownership to its client handle.
pub(super) struct AbortActorOnDrop(Option<AbortHandle>);

impl AbortActorOnDrop {
    pub(super) fn new(actor: AbortHandle) -> Self {
        Self(Some(actor))
    }

    pub(super) fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for AbortActorOnDrop {
    fn drop(&mut self) {
        if let Some(actor) = self.0.take() {
            actor.abort();
        }
    }
}
