use serde::{Deserialize, Serialize};
use specta::Type;

/// Typed package Document direction relative to the proxy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolDirection {
    Upstream,
    Downstream,
}
