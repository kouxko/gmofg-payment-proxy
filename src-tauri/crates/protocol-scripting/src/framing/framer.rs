use std::sync::Arc;

use super::{
    FrameBuffer, FramingDecision, ProtocolFramingError, ProtocolFramingLimits,
    ProtocolFramingResult, ProtocolReader, validate_decision,
};

/// 把当前 FIFO 快照裁决成一个切帧结果。
pub(crate) trait FrameDecider {
    fn decide(&mut self, reader: ProtocolReader) -> ProtocolFramingResult<FramingDecision>;
}

impl<F> FrameDecider for F
where
    F: FnMut(ProtocolReader) -> ProtocolFramingResult<FramingDecision>,
{
    fn decide(&mut self, reader: ProtocolReader) -> ProtocolFramingResult<FramingDecision> {
        self(reader)
    }
}

/// 单连接、单方向的有界切帧器。
///
/// 每个实例独占一个 [`FrameBuffer`] 和一个脚本裁决器。外层需要为 Upstream 与 Downstream 分别
/// 创建实例；类型中没有共享全局状态，因此两个方向不会串 FIFO、Reader 或脚本调用上下文。
pub(crate) struct SingleDirectionFramer<D> {
    decider: D,
    buffer: FrameBuffer,
    limits: ProtocolFramingLimits,
}

impl<D: FrameDecider> SingleDirectionFramer<D> {
    pub(crate) fn new(decider: D, limits: ProtocolFramingLimits) -> Self {
        Self {
            decider,
            buffer: FrameBuffer::default(),
            limits,
        }
    }

    /// 追加任意 TCP chunk，并返回本次能够切出的所有完整 Frame。
    ///
    /// 大 chunk 会按 FIFO 剩余空间分段灌入。这样一个 chunk 即使粘有许多小 Frame，也不会仅因
    /// chunk 总长度超过 FIFO 上限而误报；只有脚本无法给出合法可消费前缀时才会按对应的 Frame
    /// 决策错误关闭本方向。
    pub(crate) fn push(&mut self, chunk: Vec<u8>) -> ProtocolFramingResult<Vec<Vec<u8>>> {
        let chunk: Arc<[u8]> = chunk.into();
        let mut offset = 0;
        let mut frames = Vec::new();

        while offset < chunk.len() {
            let room = self.limits.max_fifo_usize() - self.buffer.len();
            if room == 0 {
                return Err(
                    self.fail_and_release(ProtocolFramingError::FifoLimitExceeded {
                        maximum: self.limits.max_fifo_bytes(),
                    }),
                );
            }
            let end = offset + room.min(chunk.len() - offset);
            self.buffer.append(Arc::clone(&chunk), offset..end);
            offset = end;
            if let Err(error) = self.drain_complete_frames(&mut frames) {
                return Err(self.fail_and_release(error));
            }
        }

        Ok(frames)
    }

    /// 处理对端 EOF。空 FIFO 正常完成；残留字节一律是截断 Frame，并立即释放缓冲内存。
    pub(crate) fn finish(&mut self) -> ProtocolFramingResult<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let buffered_bytes = u64::try_from(self.buffer.len()).unwrap_or(u64::MAX);
        self.buffer.release();
        Err(ProtocolFramingError::TruncatedFrame { buffered_bytes })
    }

    /// 连接取消或错误关闭时立即释放本方向的全部 FIFO 内存。
    pub(crate) fn cancel(&mut self) {
        self.buffer.release();
    }

    #[cfg(test)]
    pub(crate) const fn buffered_bytes(&self) -> usize {
        self.buffer.len()
    }

    #[cfg(test)]
    pub(crate) fn buffer_chunk_capacity(&self) -> usize {
        self.buffer.chunk_capacity()
    }

    /// 只用于证明内部“不可能空闲在满 FIFO”不变量即使被未来改动破坏也会 fail closed。
    #[cfg(test)]
    pub(crate) fn force_full_fifo_for_invariant_test(&mut self) {
        let bytes: Arc<[u8]> = vec![0; self.limits.max_fifo_usize()].into();
        self.buffer.append(bytes, 0..self.limits.max_fifo_usize());
    }

    fn drain_complete_frames(&mut self, frames: &mut Vec<Vec<u8>>) -> ProtocolFramingResult<()> {
        while !self.buffer.is_empty() {
            let available = self.buffer.len();
            let decision = self.decider.decide(self.buffer.reader())?;
            match validate_decision(decision, available, self.limits.max_frame_usize())? {
                FramingDecision::NeedMore(_) => return Ok(()),
                FramingDecision::Complete(length) => {
                    frames.push(self.buffer.take_prefix(length));
                }
                FramingDecision::Reject(reason) => {
                    return Err(ProtocolFramingError::Rejected { reason });
                }
            }
        }
        Ok(())
    }

    fn fail_and_release(&mut self, error: ProtocolFramingError) -> ProtocolFramingError {
        self.buffer.release();
        error
    }
}
