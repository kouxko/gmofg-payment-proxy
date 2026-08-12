use crate::{ProxyError, listener::ChildTaskAggregate};

pub(crate) const CONNECTION_CHILD_TASK_PANICKED: &str = "CONNECTION_CHILD_TASK_PANICKED";
pub(crate) const LISTENER_SHUTDOWN_GRACE_EXCEEDED: &str = "LISTENER_SHUTDOWN_GRACE_EXCEEDED";

#[derive(Debug)]
pub(crate) enum PrimaryConnectionOutcome {
    Success,
    Cancelled,
    Panicked,
    Failed(ProxyError),
}

impl From<crate::Result<()>> for PrimaryConnectionOutcome {
    fn from(result: crate::Result<()>) -> Self {
        match result {
            Ok(()) => Self::Success,
            Err(error) => Self::Failed(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalConnectionOutcome {
    Success,
    Cancelled,
    Failed { code: &'static str, message: String },
    ChildTaskPanicked,
    ShutdownGraceExceeded,
}

impl TerminalConnectionOutcome {
    pub(crate) fn is_listener_fault(&self) -> bool {
        matches!(self, Self::ChildTaskPanicked | Self::ShutdownGraceExceeded)
    }
}

pub(crate) fn synthesize_terminal(
    primary: PrimaryConnectionOutcome,
    children: ChildTaskAggregate,
    forced_abort: bool,
    cancellation_observed: bool,
) -> TerminalConnectionOutcome {
    if children.panic_seen || matches!(primary, PrimaryConnectionOutcome::Panicked) {
        return TerminalConnectionOutcome::ChildTaskPanicked;
    }
    if forced_abort {
        return TerminalConnectionOutcome::ShutdownGraceExceeded;
    }
    if cancellation_observed {
        return TerminalConnectionOutcome::Cancelled;
    }
    match primary {
        PrimaryConnectionOutcome::Cancelled => TerminalConnectionOutcome::Cancelled,
        PrimaryConnectionOutcome::Panicked => TerminalConnectionOutcome::ChildTaskPanicked,
        PrimaryConnectionOutcome::Failed(error) => TerminalConnectionOutcome::Failed {
            code: error.code,
            message: error.message,
        },
        PrimaryConnectionOutcome::Success => match children.lowest_error {
            Some((_, error)) => TerminalConnectionOutcome::Failed {
                code: error.code,
                message: error.message,
            },
            None => TerminalConnectionOutcome::Success,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::ErrorCode;

    use super::*;

    #[test]
    fn terminal_precedence_is_deterministic() {
        let children = ChildTaskAggregate {
            panic_seen: true,
            ..ChildTaskAggregate::default()
        };
        assert_eq!(
            synthesize_terminal(PrimaryConnectionOutcome::Success, children, true, true),
            TerminalConnectionOutcome::ChildTaskPanicked
        );
        assert_eq!(
            synthesize_terminal(
                PrimaryConnectionOutcome::Failed(ProxyError::new(ErrorCode::Io, "primary")),
                ChildTaskAggregate::default(),
                true,
                true,
            ),
            TerminalConnectionOutcome::ShutdownGraceExceeded
        );
        assert_eq!(
            synthesize_terminal(
                PrimaryConnectionOutcome::Failed(ProxyError::new(ErrorCode::Io, "primary")),
                ChildTaskAggregate::default(),
                false,
                true,
            ),
            TerminalConnectionOutcome::Cancelled
        );
    }
}
