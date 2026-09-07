use std::sync::Arc;

#[cfg(test)]
use std::net::SocketAddr;

use super::MCP_PORT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // Exact public warning names are all IPv6-specific.
pub(crate) enum McpTransportWarningCode {
    Ipv6Unsupported,
    Ipv6DualStackCovered,
    Ipv6Degraded,
}

impl McpTransportWarningCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Ipv6Unsupported => "ipv6_unsupported",
            Self::Ipv6DualStackCovered => "ipv6_dual_stack_covered",
            Self::Ipv6Degraded => "IPV6_DEGRADED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpIpCapability {
    available: bool,
    bind_address: &'static str,
    port: u16,
    warning_codes: Arc<[McpTransportWarningCode]>,
}

impl McpIpCapability {
    pub(crate) const fn available(&self) -> bool {
        self.available
    }

    pub(crate) const fn bind_address(&self) -> &'static str {
        self.bind_address
    }

    pub(crate) const fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn warning_codes(&self) -> &[McpTransportWarningCode] {
        &self.warning_codes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpTransportCapabilities {
    ipv4: McpIpCapability,
    ipv6: McpIpCapability,
    warnings: Arc<[McpTransportWarningCode]>,
}

impl McpTransportCapabilities {
    pub(crate) const fn ipv4(&self) -> &McpIpCapability {
        &self.ipv4
    }

    pub(crate) const fn ipv6(&self) -> &McpIpCapability {
        &self.ipv6
    }

    pub(crate) fn warnings(&self) -> &[McpTransportWarningCode] {
        &self.warnings
    }

    pub(super) fn production(ipv6: Ipv6BindOutcome) -> Self {
        let warning = ipv6.warning();
        let warning_codes: Arc<[McpTransportWarningCode]> =
            warning.into_iter().collect::<Vec<_>>().into();
        Self {
            ipv4: McpIpCapability {
                available: true,
                bind_address: "0.0.0.0",
                port: MCP_PORT,
                warning_codes: Arc::from([]),
            },
            ipv6: McpIpCapability {
                available: ipv6.available(),
                bind_address: "[::]",
                port: MCP_PORT,
                warning_codes: Arc::clone(&warning_codes),
            },
            warnings: warning_codes,
        }
    }

    #[cfg(test)]
    pub(super) fn test(address: SocketAddr) -> Self {
        let ipv4_available = address.is_ipv4();
        Self {
            ipv4: McpIpCapability {
                available: ipv4_available,
                bind_address: "0.0.0.0",
                port: address.port(),
                warning_codes: Arc::from([]),
            },
            ipv6: McpIpCapability {
                available: !ipv4_available,
                bind_address: "[::]",
                port: address.port(),
                warning_codes: Arc::from([]),
            },
            warnings: Arc::from([]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ipv6BindOutcome {
    Independent,
    DualStackCovered,
    Unsupported,
    Degraded,
}

impl Ipv6BindOutcome {
    const fn available(self) -> bool {
        matches!(self, Self::Independent | Self::DualStackCovered)
    }

    const fn warning(self) -> Option<McpTransportWarningCode> {
        match self {
            Self::Independent => None,
            Self::DualStackCovered => Some(McpTransportWarningCode::Ipv6DualStackCovered),
            Self::Unsupported => Some(McpTransportWarningCode::Ipv6Unsupported),
            Self::Degraded => Some(McpTransportWarningCode::Ipv6Degraded),
        }
    }
}
