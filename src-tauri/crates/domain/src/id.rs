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
uuid_id!(CertificateId);
uuid_id!(EventId);

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
