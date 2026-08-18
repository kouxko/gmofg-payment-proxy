//! 强类型标识符。
//!
//! 会话、报文、断点、连接和规则底层都使用 UUID，但业务含义不同。分别包装后，编译器
//! 可以阻止“把会话 ID 误传成规则 ID”这类低级错误。

use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(RuntimeEpoch);
uuid_id!(ConnectionId);
uuid_id!(SessionId);
uuid_id!(MessageId);
uuid_id!(BreakpointId);
uuid_id!(RuleId);
uuid_id!(SocketDocumentRuleId);
uuid_id!(CertificateId);
uuid_id!(EventId);
uuid_id!(WorkspaceId);
uuid_id!(ListenerId);
uuid_id!(ResponseAssertionId);
uuid_id!(FaultPresetId);
uuid_id!(CertificateReferenceId);

#[cfg(test)]
mod tests {
    use super::*;

    // DATA-001, STATE-009
    #[test]
    fn strongly_typed_ids_are_generated_by_rust_and_do_not_alias() {
        assert_ne!(SessionId::new(), SessionId::new());
        let uuid = Uuid::new_v4();
        assert_eq!(MessageId::from_uuid(uuid).as_uuid(), uuid);
    }
}
