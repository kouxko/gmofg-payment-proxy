use intercept_proxy_domain::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::num::NonZeroUsize;

/// Positive number of bytes consumed by one complete frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct ConsumedBytes(NonZeroUsize);

impl ConsumedBytes {
    /// Creates a positive consumed-byte count.
    pub fn new(value: usize) -> Result<Self, DomainError> {
        NonZeroUsize::new(value).map(Self).ok_or_else(|| {
            DomainError::new(
                ErrorCode::ProtocolPackageInvalid,
                "FrameResult complete requires consumedBytes greater than zero",
            )
            .with_field_error("consumedBytes", "must be greater than zero")
        })
    }

    /// Returns the validated positive byte count.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

impl<'de> Deserialize<'de> for ConsumedBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(usize::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Closed result of one fixed frame hook.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FrameResult {
    /// The current buffer does not yet contain one complete frame.
    NeedMore {
        /// Optional byte count needed before trying the frame hook again.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        required_bytes: Option<usize>,
    },
    /// The buffer prefix is one complete frame.
    Complete {
        /// Positive byte count consumed from the buffer prefix.
        consumed_bytes: ConsumedBytes,
    },
    /// The package rejects the current buffer as an invalid frame.
    Reject {
        /// Package-supplied rejection reason.
        reason: String,
    },
}

#[derive(Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum FrameResultWire {
    NeedMore {
        #[serde(default)]
        required_bytes: Option<usize>,
    },
    Complete {
        consumed_bytes: usize,
    },
    Reject {
        reason: String,
    },
}

impl TryFrom<FrameResultWire> for FrameResult {
    type Error = DomainError;

    fn try_from(value: FrameResultWire) -> Result<Self, Self::Error> {
        match value {
            FrameResultWire::NeedMore { required_bytes } => Ok(Self::NeedMore { required_bytes }),
            FrameResultWire::Complete { consumed_bytes } => Self::complete(consumed_bytes),
            FrameResultWire::Reject { reason } => Ok(Self::Reject { reason }),
        }
    }
}

impl FrameResult {
    /// Creates a complete result whose consumed byte count is positive by construction.
    pub fn complete(consumed_bytes: usize) -> Result<Self, DomainError> {
        Ok(Self::Complete {
            consumed_bytes: ConsumedBytes::new(consumed_bytes)?,
        })
    }

    /// Validates the adapter-context invariant against the current accumulated buffer.
    pub fn validate_against_buffer_len(&self, buffer_len: usize) -> Result<(), DomainError> {
        if let Self::Complete { consumed_bytes } = self
            && consumed_bytes.get() > buffer_len
        {
            return Err(DomainError::new(
                ErrorCode::ProtocolPackageInvalid,
                "FrameResult consumedBytes exceeds the current buffer length",
            )
            .with_field_error("consumedBytes", "must not exceed buffer length"));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for FrameResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        FrameResultWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}
