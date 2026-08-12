mod admission;
mod context;
mod handler;
mod outcome;
mod supervisor;
mod task_scope;

pub(crate) use admission::{AdmissionDecision, ListenerAdmission, ListenerCapacity};
pub(crate) use context::ListenerRunContext;
#[allow(unused_imports)]
pub(crate) use handler::{
    ConnectionHandler, ConnectionLifecycleObserver, ListenerRejection, sealed,
};
pub(crate) use outcome::{
    CONNECTION_CHILD_TASK_PANICKED, LISTENER_SHUTDOWN_GRACE_EXCEEDED, PrimaryConnectionOutcome,
    TerminalConnectionOutcome,
};
#[allow(unused_imports)]
pub(crate) use supervisor::{
    ListenerConfig, ListenerRunOutcome, ListenerSupervisor, NoopConnectionLifecycleObserver,
};

#[allow(unused_imports)]
pub(crate) use task_scope::{
    ChildTaskAggregate, ChildTaskError, ChildTaskId, ConnectionTaskScope, ScopePhase,
    SpawnRejected, TaskScopeSnapshot,
};
