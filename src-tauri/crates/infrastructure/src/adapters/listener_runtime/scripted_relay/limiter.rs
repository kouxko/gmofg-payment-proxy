//! Scripted 阻塞命令的进程级与单 Listener 双层许可。

use std::sync::{Arc, OnceLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};

use super::DirectionCommand;

const GLOBAL_SCRIPTED_BLOCKING_COMMAND_LIMIT: usize = 64;

#[derive(Clone)]
pub(in crate::adapters::listener_runtime) struct BlockingCommandSlots {
    global: Arc<Semaphore>,
    listener: Arc<Semaphore>,
}

impl BlockingCommandSlots {
    pub(in crate::adapters::listener_runtime) fn new_relay(maximum_connections: u16) -> Self {
        Self::new(maximum_connections, 2)
    }

    fn new(maximum_connections: u16, directions_per_connection: usize) -> Self {
        static GLOBAL: OnceLock<Arc<Semaphore>> = OnceLock::new();
        let global = Arc::clone(
            GLOBAL.get_or_init(|| Arc::new(Semaphore::new(GLOBAL_SCRIPTED_BLOCKING_COMMAND_LIMIT))),
        );
        Self::from_global(maximum_connections, directions_per_connection, global)
    }

    #[cfg(test)]
    pub(super) fn with_global(maximum_connections: u16, global: Arc<Semaphore>) -> Self {
        Self::from_global(maximum_connections, 2, global)
    }

    fn from_global(
        maximum_connections: u16,
        directions_per_connection: usize,
        global: Arc<Semaphore>,
    ) -> Self {
        // Relay 每连接两个方向，LocalResponder 每连接一个 exchange；二者分别只允许当前
        // maximum_connections 对应的阻塞脚本数，避免关闭连接后的协作取消期间继续积压。
        let listener_limit = usize::from(maximum_connections) * directions_per_connection;
        Self {
            global,
            listener: Arc::new(Semaphore::new(listener_limit)),
        }
    }

    async fn acquire(&self) -> Option<BlockingCommandPermits> {
        // 所有调用统一先拿 Listener、再拿全局许可，避免跨 Listener 的反向锁序。
        let listener = Arc::clone(&self.listener).acquire_owned().await.ok()?;
        let global = Arc::clone(&self.global).acquire_owned().await.ok()?;
        Some(BlockingCommandPermits {
            _listener: listener,
            _global: global,
        })
    }
}

pub(in crate::adapters::listener_runtime) struct BlockingCommandPermits {
    _listener: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

pub(super) async fn acquire_command_permits(
    slots: &BlockingCommandSlots,
    command: &mut DirectionCommand,
) -> Option<BlockingCommandPermits> {
    match command {
        DirectionCommand::Frame { reply, .. } => acquire_for_reply(slots, reply).await,
        DirectionCommand::Decode { reply, .. } => acquire_for_reply(slots, reply).await,
        DirectionCommand::Display { reply, .. } => acquire_for_reply(slots, reply).await,
        DirectionCommand::Encode { reply, .. } => acquire_for_reply(slots, reply).await,
    }
}

pub(in crate::adapters::listener_runtime) async fn acquire_for_reply<T>(
    slots: &BlockingCommandSlots,
    reply: &mut oneshot::Sender<T>,
) -> Option<BlockingCommandPermits> {
    tokio::select! {
        biased;
        () = reply.closed() => None,
        permits = slots.acquire() => permits,
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use tokio::sync::{Semaphore, oneshot};

    use super::{BlockingCommandSlots, acquire_for_reply};

    #[tokio::test]
    async fn global_and_listener_limits_bound_detached_blocking_work() {
        let global = Arc::new(Semaphore::new(2));
        let first_listener = BlockingCommandSlots::with_global(1, Arc::clone(&global));
        let second_listener = BlockingCommandSlots::with_global(1, Arc::clone(&global));
        let first = first_listener.acquire().await.unwrap();
        let second = first_listener.acquire().await.unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(20), second_listener.acquire())
                .await
                .is_err(),
            "跨 Listener 的第三条阻塞命令必须等待进程级许可"
        );
        drop(first);
        assert!(
            tokio::time::timeout(Duration::from_millis(200), second_listener.acquire())
                .await
                .unwrap()
                .is_some()
        );
        drop(second);
    }

    #[tokio::test]
    async fn dropped_reply_skips_the_blocking_pool_before_taking_a_slot() {
        let global = Arc::new(Semaphore::new(1));
        let slots = BlockingCommandSlots::with_global(1, Arc::clone(&global));
        let (mut reply, receive) = oneshot::channel::<()>();
        drop(receive);

        assert!(acquire_for_reply(&slots, &mut reply).await.is_none());
        assert_eq!(global.available_permits(), 1);
    }
}
