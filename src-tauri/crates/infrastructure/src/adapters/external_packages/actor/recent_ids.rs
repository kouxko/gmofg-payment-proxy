//! JSON-RPC 响应关联使用的有界墓碑窗口。

use std::collections::{HashSet, VecDeque};

/// 只保留近期完成/取消 ID，连接内存不随累计调用数增长。
pub(in crate::adapters::external_packages) struct RecentRequestIds {
    capacity: usize,
    order: VecDeque<String>,
    ids: HashSet<String>,
}

impl RecentRequestIds {
    pub(in crate::adapters::external_packages) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::with_capacity(capacity),
            ids: HashSet::with_capacity(capacity),
        }
    }

    pub(in crate::adapters::external_packages) fn insert(&mut self, request_id: String) {
        if !self.ids.insert(request_id.clone()) {
            return;
        }
        self.order.push_back(request_id);
        while self.order.len() > self.capacity {
            if let Some(expired) = self.order.pop_front() {
                self.ids.remove(&expired);
            }
        }
    }

    pub(in crate::adapters::external_packages) fn contains(&self, request_id: &str) -> bool {
        self.ids.contains(request_id)
    }

    pub(in crate::adapters::external_packages) fn remove(&mut self, request_id: &str) -> bool {
        self.ids.remove(request_id)
    }
}
