//! 乐观并发版本号。
//!
//! 调用方提交自己读取时的版本，领域层验证一致后才允许写入，避免 UI 编辑期间实体已
//! 被别的任务更新却被旧数据静默覆盖。

use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;

/// JavaScript `number` 能精确往返的最大非负整数（2^53 - 1）。
///
/// 需要通过 JSON/TypeScript 暴露的 `u64` 领域字段应在各自聚合边界复用此上限。
pub const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    pub const INITIAL: Self = Self(1);

    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn verify(self, expected: Self) -> Result<(), DomainError> {
        if self == expected {
            Ok(())
        } else {
            Err(DomainError::new(
                ErrorCode::RevisionConflict,
                "实体已被其他操作更新",
            ))
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// 计算下一个 revision；`u64` 已耗尽时返回冲突错误，不返回重复 revision。
    pub fn checked_next(self) -> Result<Self, DomainError> {
        self.0.checked_add(1).map(Self).ok_or_else(|| {
            DomainError::new(
                ErrorCode::RevisionConflict,
                "revision 已达到上限，无法继续更新实体",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // STATE-015, ENGINE-008
    #[test]
    fn optimistic_revision_rejects_stale_writes_and_advances() {
        let current = Revision::new(4);
        assert!(current.verify(Revision::new(4)).is_ok());
        assert_eq!(
            current.verify(Revision::new(3)).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(current.next(), Revision::new(5));
        assert_eq!(current.checked_next().unwrap(), Revision::new(5));
        assert_eq!(
            Revision::new(u64::MAX).checked_next().unwrap_err().code,
            ErrorCode::RevisionConflict
        );
    }
}
