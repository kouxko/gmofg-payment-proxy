use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;

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
    }
}
