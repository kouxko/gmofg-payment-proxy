use std::{ops::Range, sync::Arc};

use rhai::{Blob, Engine, EvalAltResult, INT, Position};

use super::{ProtocolFramingError, ProtocolFramingResult};

/// FIFO 中一个不可变 chunk 的共享切片。
///
/// 网络 chunk 只被 `Arc` 持有一次。Reader 快照复制的只是 `Arc` 与范围，不复制整个已缓冲报文。
#[derive(Clone, Debug)]
pub(super) struct ReaderSegment {
    bytes: Arc<[u8]>,
    range: Range<usize>,
}

impl ReaderSegment {
    pub(super) fn new(bytes: Arc<[u8]>, range: Range<usize>) -> Self {
        Self { bytes, range }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[self.range.clone()]
    }
}

/// 一次 `frame()` 调用期间只读的单方向 FIFO 视图。
///
/// 类型不提供写入或消费方法。脚本只能通过已注册的读取函数观察当前快照；每次入口调用使用新
/// Scope，返回值又必须是 `FramingDecision`，因此 Reader 无法跨调用保存为宿主状态。
#[derive(Clone, Debug)]
pub(crate) struct ProtocolReader {
    segments: Arc<[ReaderSegment]>,
    available: usize,
}

impl ProtocolReader {
    pub(super) fn from_segments(segments: Vec<ReaderSegment>, available: usize) -> Self {
        Self {
            segments: segments.into(),
            available,
        }
    }

    pub(super) fn empty() -> Self {
        Self::from_segments(Vec::new(), 0)
    }

    pub(crate) const fn available(&self) -> usize {
        self.available
    }

    pub(crate) fn peek(&self, offset: usize, length: usize) -> ProtocolFramingResult<Vec<u8>> {
        let range = checked_range(offset, length, self.available)?;
        Ok(self.copy_range(range))
    }

    pub(crate) fn peek_u8(&self, offset: usize) -> ProtocolFramingResult<u8> {
        self.byte_at(offset)
            .ok_or(ProtocolFramingError::ReaderOutOfBounds)
    }

    pub(crate) fn peek_u16_be(&self, offset: usize) -> ProtocolFramingResult<u16> {
        self.peek_array::<2>(offset).map(u16::from_be_bytes)
    }

    pub(crate) fn peek_u16_le(&self, offset: usize) -> ProtocolFramingResult<u16> {
        self.peek_array::<2>(offset).map(u16::from_le_bytes)
    }

    pub(crate) fn peek_u32_be(&self, offset: usize) -> ProtocolFramingResult<u32> {
        self.peek_array::<4>(offset).map(u32::from_be_bytes)
    }

    pub(crate) fn peek_u32_le(&self, offset: usize) -> ProtocolFramingResult<u32> {
        self.peek_array::<4>(offset).map(u32::from_le_bytes)
    }

    pub(crate) fn find(
        &self,
        pattern: &[u8],
        start: usize,
    ) -> ProtocolFramingResult<Option<usize>> {
        if pattern.is_empty() {
            return Err(ProtocolFramingError::EmptyFindPattern);
        }
        if start >= self.available {
            return Err(ProtocolFramingError::InvalidFindStart);
        }
        if pattern.len() > self.available - start {
            return Ok(None);
        }

        // 数据可能跨多个网络 chunk；逐 byte 比较避免为了 find 把整个 FIFO 再复制成连续 Vec。
        let last_start = self.available - pattern.len();
        for candidate in start..=last_start {
            if pattern
                .iter()
                .enumerate()
                .all(|(index, expected)| self.byte_at(candidate + index) == Some(*expected))
            {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    fn peek_array<const N: usize>(&self, offset: usize) -> ProtocolFramingResult<[u8; N]> {
        let bytes = self.peek(offset, N)?;
        bytes
            .try_into()
            .map_err(|_| ProtocolFramingError::ReaderOutOfBounds)
    }

    fn byte_at(&self, mut offset: usize) -> Option<u8> {
        if offset >= self.available {
            return None;
        }
        for segment in self.segments.iter() {
            let bytes = segment.as_slice();
            if offset < bytes.len() {
                return Some(bytes[offset]);
            }
            offset -= bytes.len();
        }
        None
    }

    fn copy_range(&self, range: Range<usize>) -> Vec<u8> {
        let mut result = Vec::with_capacity(range.len());
        let mut logical_start = 0;
        for segment in self.segments.iter() {
            let bytes = segment.as_slice();
            let logical_end = logical_start + bytes.len();
            let copy_start = range.start.max(logical_start);
            let copy_end = range.end.min(logical_end);
            if copy_start < copy_end {
                result.extend_from_slice(
                    &bytes[copy_start - logical_start..copy_end - logical_start],
                );
            }
            if logical_end >= range.end {
                break;
            }
            logical_start = logical_end;
        }
        result
    }
}

pub(super) fn register(engine: &mut Engine) {
    engine
        .register_type_with_name::<ProtocolReader>("Reader")
        .register_fn("available", |reader: &mut ProtocolReader| {
            usize_to_int(reader.available())
        })
        .register_fn("peek", rhai_peek)
        .register_fn("peek_u8", rhai_peek_u8)
        .register_fn("peek_u16_be", rhai_peek_u16_be)
        .register_fn("peek_u16_le", rhai_peek_u16_le)
        .register_fn("peek_u32_be", rhai_peek_u32_be)
        .register_fn("peek_u32_le", rhai_peek_u32_le)
        .register_fn("find", rhai_find);
}

fn rhai_peek(
    reader: &mut ProtocolReader,
    offset: INT,
    length: INT,
) -> Result<Blob, Box<EvalAltResult>> {
    reader
        .peek(int_to_usize(offset)?, int_to_usize(length)?)
        .map_err(|error| Box::new(framing_error_to_rhai(&error)))
}

fn rhai_peek_u8(reader: &mut ProtocolReader, offset: INT) -> Result<INT, Box<EvalAltResult>> {
    reader
        .peek_u8(int_to_usize(offset)?)
        .map(INT::from)
        .map_err(|error| Box::new(framing_error_to_rhai(&error)))
}

fn rhai_peek_u16_be(reader: &mut ProtocolReader, offset: INT) -> Result<INT, Box<EvalAltResult>> {
    reader
        .peek_u16_be(int_to_usize(offset)?)
        .map(INT::from)
        .map_err(|error| Box::new(framing_error_to_rhai(&error)))
}

fn rhai_peek_u16_le(reader: &mut ProtocolReader, offset: INT) -> Result<INT, Box<EvalAltResult>> {
    reader
        .peek_u16_le(int_to_usize(offset)?)
        .map(INT::from)
        .map_err(|error| Box::new(framing_error_to_rhai(&error)))
}

fn rhai_peek_u32_be(reader: &mut ProtocolReader, offset: INT) -> Result<INT, Box<EvalAltResult>> {
    reader
        .peek_u32_be(int_to_usize(offset)?)
        .map(INT::from)
        .map_err(|error| Box::new(framing_error_to_rhai(&error)))
}

fn rhai_peek_u32_le(reader: &mut ProtocolReader, offset: INT) -> Result<INT, Box<EvalAltResult>> {
    reader
        .peek_u32_le(int_to_usize(offset)?)
        .map(INT::from)
        .map_err(|error| Box::new(framing_error_to_rhai(&error)))
}

// Rhai 的原生函数注册不接受 `&mut Blob` 作为第二个脚本参数，只能按值接收 Dynamic 中的 Blob。
// 这里仍然只借用其字节，不复制 pattern。
#[allow(clippy::needless_pass_by_value)]
fn rhai_find(
    reader: &mut ProtocolReader,
    pattern: Blob,
    start: INT,
) -> Result<INT, Box<EvalAltResult>> {
    reader
        .find(&pattern, int_to_usize(start)?)
        .map(|offset| offset.map_or(-1, usize_to_int))
        .map_err(|error| Box::new(framing_error_to_rhai(&error)))
}

fn checked_range(
    offset: usize,
    length: usize,
    available: usize,
) -> ProtocolFramingResult<Range<usize>> {
    let end = offset
        .checked_add(length)
        .ok_or(ProtocolFramingError::ReaderOutOfBounds)?;
    if offset > available || end > available {
        Err(ProtocolFramingError::ReaderOutOfBounds)
    } else {
        Ok(offset..end)
    }
}

fn int_to_usize(value: INT) -> Result<usize, Box<EvalAltResult>> {
    usize::try_from(value).map_err(|_| {
        Box::new(framing_error_to_rhai(
            &ProtocolFramingError::ReaderOutOfBounds,
        ))
    })
}

fn usize_to_int(value: usize) -> INT {
    INT::try_from(value).unwrap_or(INT::MAX)
}

fn framing_error_to_rhai(error: &ProtocolFramingError) -> EvalAltResult {
    EvalAltResult::ErrorRuntime(format!("{:?}", error.code()).into(), Position::NONE)
}
