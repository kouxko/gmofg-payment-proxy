use super::{
    BTreeMap, BTreeSet, Deserialize, Deserializer, Duration, ErrorCode, FromStr, MessageLimits,
    ProxyError, Result, Serialize, SocketAddr, Uuid, fmt,
};

/// Stable, product-neutral identifier for one configured proxy channel.
///
/// IDs are intentionally safe for logs, configuration keys and command-line
/// arguments: 1-64 ASCII characters, beginning and ending with an
/// alphanumeric character, with `-`, `_` and `.` allowed internally.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ChannelId(String);

impl ChannelId {
    pub const MAX_LEN: usize = 64;

    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        Self::validate(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(value: &str) -> Result<()> {
        let valid_length = !value.is_empty() && value.len() <= Self::MAX_LEN;
        let mut chars = value.chars();
        let first = chars.next();
        let last = value.chars().next_back();
        let valid_edges = first.is_some_and(|character| character.is_ascii_alphanumeric())
            && last.is_some_and(|character| character.is_ascii_alphanumeric());
        let valid_characters = value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if valid_length && valid_edges && valid_characters {
            return Ok(());
        }
        Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            format!(
                "invalid channel ID {value:?}; expected 1-{} ASCII letters, digits, '-', '_' or '.', with alphanumeric edges",
                Self::MAX_LEN
            ),
        ))
    }
}

impl AsRef<str> for ChannelId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ChannelId {
    type Err = ProxyError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl TryFrom<String> for ChannelId {
    type Error = ProxyError;

    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}

impl TryFrom<&str> for ChannelId {
    type Error = ProxyError;

    fn try_from(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ChannelId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub channel: ChannelId,
    pub enabled: bool,
    pub listen_addr: SocketAddr,
    pub upstream_url: String,
}

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub channels: Vec<ChannelConfig>,
    pub limits: MessageLimits,
    pub max_connections: usize,
    pub connect_timeout: Duration,
    pub write_timeout: Duration,
    pub read_timeout: Duration,
    pub rewrite_host: bool,
    pub leaf_sans: Vec<String>,
}

impl ProxyConfig {
    pub fn validate(&self) -> Result<()> {
        let enabled: Vec<_> = self
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .collect();
        if enabled.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "at least one proxy channel must be enabled",
            ));
        }
        if enabled
            .iter()
            .any(|channel| channel.upstream_url.trim().is_empty())
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "enabled channels require an upstream URL",
            ));
        }
        if self.connect_timeout.is_zero()
            || self.write_timeout.is_zero()
            || self.read_timeout.is_zero()
            || self.limits.max_body_bytes == 0
            || self.max_connections == 0
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "timeouts, body limit, and connection limit must be greater than zero",
            ));
        }
        let mut channel_ids = BTreeSet::new();
        if self
            .channels
            .iter()
            .any(|channel| !channel_ids.insert(&channel.channel))
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "each channel may appear at most once",
            ));
        }
        let mut listen_addresses = BTreeSet::new();
        if enabled.iter().any(|channel| {
            channel.listen_addr.port() != 0 && !listen_addresses.insert(channel.listen_addr)
        }) {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "enabled channels cannot use the same listen address",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub state: ProxyState,
    pub runtime_epoch: Option<Uuid>,
    pub listeners: BTreeMap<ChannelId, SocketAddr>,
    pub fault: Option<String>,
}
