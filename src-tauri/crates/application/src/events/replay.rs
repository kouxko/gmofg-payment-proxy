use std::{collections::VecDeque, sync::Arc};

use crate::{CapacityLedger, UiEventEnvelope};

#[derive(Debug, Clone)]
/// 按游标读取历史事件的结果。
///
/// `snapshot_required` 为真表示请求位置太旧，调用方必须重新查询页面快照。
pub struct EventReplay {
    pub events: Vec<UiEventEnvelope>,
    pub current_cursor: u64,
    pub snapshot_required: bool,
}

/// 带内存记账的历史回放集合。
///
/// 克隆事件在外层真正消费前仍占用容量；回调返回后才释放对应字节。未发送部分会在
/// `Drop` 时自动归还，避免 Tauri 序列化期间出现未记账副本。
#[derive(Debug)]
pub struct TrackedReplay {
    events: VecDeque<UiEventEnvelope>,
    reserved_logical_bytes: u64,
    capacity: Arc<CapacityLedger>,
}

impl TrackedReplay {
    pub(super) fn new(events: Vec<UiEventEnvelope>, capacity: Arc<CapacityLedger>) -> Self {
        let reserved_logical_bytes = events.iter().map(UiEventEnvelope::logical_bytes).sum();
        Self {
            events: events.into(),
            reserved_logical_bytes,
            capacity,
        }
    }

    pub fn drain_with<E>(
        &mut self,
        mut consume: impl FnMut(UiEventEnvelope) -> Result<(), E>,
    ) -> Result<(), E> {
        while let Some(event) = self.events.pop_front() {
            let bytes = event.logical_bytes();
            let result = consume(event);
            self.reserved_logical_bytes = self.reserved_logical_bytes.saturating_sub(bytes);
            self.capacity.release_event_bytes(bytes);
            result?;
        }
        Ok(())
    }
}

impl Drop for TrackedReplay {
    fn drop(&mut self) {
        self.capacity
            .release_event_bytes(self.reserved_logical_bytes);
        self.reserved_logical_bytes = 0;
    }
}
