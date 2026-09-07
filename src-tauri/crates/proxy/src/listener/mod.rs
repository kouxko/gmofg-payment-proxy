mod admission;
mod context;
mod handler;
mod outcome;
mod supervisor;
mod task_scope;

pub(crate) use admission::{AdmissionDecision, ListenerAdmission, ListenerCapacity};
pub(crate) use context::ListenerRunContext;
pub(crate) use handler::{
    ConnectionHandler, ConnectionLifecycleObserver, ListenerRejection, sealed,
};
pub(crate) use outcome::{
    CONNECTION_CHILD_TASK_PANICKED, LISTENER_SHUTDOWN_GRACE_EXCEEDED, PrimaryConnectionOutcome,
    TerminalConnectionOutcome,
};
pub(crate) use supervisor::{ListenerConfig, ListenerSupervisor, NoopConnectionLifecycleObserver};

pub(crate) use task_scope::{ChildTaskAggregate, ChildTaskError, ConnectionTaskScope};
