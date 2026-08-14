use std::{collections::VecDeque, ops::Range, sync::Arc};

use super::{ProtocolReader, ReaderSegment};

#[derive(Debug)]
struct BufferedChunk {
    bytes: Arc<[u8]>,
    range: Range<usize>,
}

/// 单方向 FIFO。它只有一个可变所有者，不使用锁，也不会在 Upstream/Downstream 之间共享。
#[derive(Debug, Default)]
pub(super) struct FrameBuffer {
    chunks: VecDeque<BufferedChunk>,
    len: usize,
}

impl FrameBuffer {
    pub(super) const fn len(&self) -> usize {
        self.len
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn append(&mut self, bytes: Arc<[u8]>, range: Range<usize>) {
        let length = range.len();
        if length == 0 {
            return;
        }
        self.chunks.push_back(BufferedChunk { bytes, range });
        self.len += length;
    }

    pub(super) fn reader(&self) -> ProtocolReader {
        let segments = self
            .chunks
            .iter()
            .map(|chunk| ReaderSegment::new(Arc::clone(&chunk.bytes), chunk.range.clone()))
            .collect();
        ProtocolReader::from_segments(segments, self.len)
    }

    pub(super) fn take_prefix(&mut self, length: usize) -> Vec<u8> {
        debug_assert!(length <= self.len);
        let mut remaining = length;
        let mut frame = Vec::with_capacity(length);
        while remaining > 0 {
            let chunk = self
                .chunks
                .front_mut()
                .expect("buffer length and chunk queue must remain consistent");
            let available = chunk.range.len();
            let take = remaining.min(available);
            frame.extend_from_slice(&chunk.bytes[chunk.range.start..chunk.range.start + take]);
            chunk.range.start += take;
            remaining -= take;
            if chunk.range.is_empty() {
                self.chunks.pop_front();
            }
        }
        self.len -= length;
        frame
    }

    /// 丢弃连接方向状态，并把 `VecDeque` 本身的预留容量一并释放。
    pub(super) fn release(&mut self) {
        *self = Self::default();
    }

    #[cfg(test)]
    pub(super) fn chunk_capacity(&self) -> usize {
        self.chunks.capacity()
    }
}
