//! Stable, read-only resources exposed by the MCP adapter.

use rmcp::model::Resource;

pub const AUTHORING_GUIDE_URI: &str = "intercept-proxy://docs/protocol-package-authoring/1.0";
pub const HOST_API_URI: &str = "intercept-proxy://docs/protocol-package-host-api/1.0";
pub const SOCKET_AUTHORING_URI: &str =
    "intercept-proxy://docs/socket-protocol-package-authoring/1.0";
pub const CERTIFICATE_CONCEPTS_URI: &str = "intercept-proxy://docs/certificate-concepts/1.0";
pub const APP_INTEGRATION_GUIDE_URI: &str = "intercept-proxy://docs/app-integration-guide/1.0";
pub const EXTERNAL_PACKAGE_INTEGRATION_GUIDE_URI: &str =
    "intercept-proxy://docs/external-package-integration-guide/1.0";
pub const DIAGNOSTIC_ARCHITECTURE_URI: &str = "intercept-proxy://docs/diagnostic-architecture/1.0";
pub const TOOL_REFERENCE_URI: &str = "intercept-proxy://docs/tool-reference/1.0";
pub const VALIDATION_PLAYBOOK_URI: &str = "intercept-proxy://docs/validation-playbook/1.0";
pub const ISO8583_MANIFEST_URI: &str =
    "intercept-proxy://templates/iso8583-ascii-standard/1.0.0/manifest.toml";
pub const ISO8583_SCHEMA_URI: &str =
    "intercept-proxy://templates/iso8583-ascii-standard/1.0.0/document.toml";
pub const ISO8583_PROTOCOL_SOURCE_URI: &str =
    "intercept-proxy://templates/iso8583-ascii-standard/1.0.0/protocol.rhai";
pub const ISO8583_DISPLAY_SOURCE_URI: &str =
    "intercept-proxy://templates/iso8583-ascii-standard/1.0.0/display.rhai";
pub const ISO8583_LIBRARY_SOURCE_URI: &str =
    "intercept-proxy://templates/iso8583-ascii-standard/1.0.0/libraries/iso8583.rhai";
pub const ISO8583_ARCHIVE_URI: &str =
    "intercept-proxy://templates/iso8583-ascii-standard/1.0.0/archive.zip";

const RULES_AND_PROTOCOL_PACKAGES: &str =
    include_str!("../../../docs/architecture/rules-and-protocol-packages.md");
const HOST_API: &str = include_str!("../../../templates/socket-protocol/API.md");
const SOCKET_AUTHORING: &str = include_str!("../../../templates/socket-protocol/AUTHORING.md");
const CERTIFICATE_CONCEPTS: &str = include_str!("../../../docs/mcp/certificate-concepts.md");
const APP_INTEGRATION_GUIDE: &str = include_str!("../../../docs/mcp/app-integration-guide.md");
const EXTERNAL_PACKAGE_INTEGRATION_GUIDE: &str =
    include_str!("../../../docs/mcp/external-package-integration-guide.md");
const DIAGNOSTIC_ARCHITECTURE: &str = include_str!("../../../docs/mcp/diagnostic-architecture.md");
const TOOL_REFERENCE: &str = include_str!("../../../docs/mcp/tool-reference.md");
const VALIDATION_PLAYBOOK: &str = include_str!("../../../docs/mcp/validation-playbook.md");
const ISO8583_MANIFEST: &str =
    include_str!("../../../templates/socket-protocol/iso8583-standard/manifest.toml");
const ISO8583_SCHEMA: &str =
    include_str!("../../../templates/socket-protocol/iso8583-standard/document.toml");
const ISO8583_PROTOCOL_SOURCE: &str =
    include_str!("../../../templates/socket-protocol/iso8583-standard/protocol.rhai");
const ISO8583_DISPLAY_SOURCE: &str =
    include_str!("../../../templates/socket-protocol/iso8583-standard/display.rhai");
const ISO8583_LIBRARY_SOURCE: &str =
    include_str!("../../../templates/socket-protocol/iso8583-standard/libraries/iso8583.rhai");

pub fn list() -> Vec<Resource> {
    vec![
        Resource::new(AUTHORING_GUIDE_URI, "protocol-package-authoring-guide")
            .with_title("Intercept Proxy rules and protocol package boundaries")
            .with_description(
                "The application-owned HTTP/Socket package contract, direction model and execution order.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(HOST_API_URI, "protocol-package-host-api")
            .with_title("Protocol package Host API v1")
            .with_description(
                "The exact Manifest, entry-point, value, resource-limit and failure contract.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(SOCKET_AUTHORING_URI, "socket-protocol-package-authoring")
            .with_title("Socket protocol package authoring guide")
            .with_description(
                "Practical framing, field parsing, reconstruction, display and test guidance.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(CERTIFICATE_CONCEPTS_URI, "certificate-concepts")
            .with_title("Certificate concepts for proxy troubleshooting")
            .with_description(
                "A beginner-friendly guide to Root CA, server/client certificates, trust, private keys and common TLS failures.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(APP_INTEGRATION_GUIDE_URI, "app-integration-guide")
            .with_title("App integration and troubleshooting guide")
            .with_description(
                "Evidence-based App-side changes for proxy routing, Android trust, HTTP and Socket clients, with alternatives and verification steps.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(
            EXTERNAL_PACKAGE_INTEGRATION_GUIDE_URI,
            "external-package-integration-guide",
        )
        .with_title("External package integration and diagnostics guide")
        .with_description(
            "The WebSocket registration, JSON-RPC method, lifecycle, size, security and MCP troubleshooting contract for external protocol packages.",
        )
        .with_mime_type("text/markdown"),
        Resource::new(DIAGNOSTIC_ARCHITECTURE_URI, "diagnostic-architecture")
            .with_title("Diagnostic evidence and code architecture map")
            .with_description(
                "How runtime logs, structured diagnostics, captures, configuration and code ownership combine into a reproducible failure report.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(TOOL_REFERENCE_URI, "tool-reference")
            .with_title("Complete MCP tool reference: 37 reads and 5 environment tools")
            .with_description(
                "Arguments, successful structured results, errors, paging, retention and evidence boundaries for every public tool.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(VALIDATION_PLAYBOOK_URI, "validation-playbook")
            .with_title("Proxy validation and troubleshooting playbook")
            .with_description(
                "Evidence-based validation order, stop conditions and safe troubleshooting guidance for HTTP, Socket, TLS, protocol packages, Android and environment candidates.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(ISO8583_MANIFEST_URI, "official-iso8583-manifest")
            .with_title("ISO 8583:1987 ASCII Profile manifest")
            .with_description("The exact manifest compiled into the official 1.0.0 template.")
            .with_mime_type("application/toml"),
        Resource::new(ISO8583_SCHEMA_URI, "official-iso8583-schema")
            .with_title("ISO 8583:1987 ASCII Profile Document Schema")
            .with_mime_type("application/toml"),
        Resource::new(ISO8583_PROTOCOL_SOURCE_URI, "official-iso8583-protocol-source")
            .with_title("ISO 8583:1987 framing, decode and encode source")
            .with_mime_type("text/x-rhai"),
        Resource::new(ISO8583_DISPLAY_SOURCE_URI, "official-iso8583-display-source")
            .with_title("ISO 8583:1987 display source")
            .with_mime_type("text/x-rhai"),
        Resource::new(ISO8583_LIBRARY_SOURCE_URI, "official-iso8583-library-source")
            .with_title("ISO 8583:1987 shared source library")
            .with_mime_type("text/x-rhai"),
        Resource::new(ISO8583_ARCHIVE_URI, "official-iso8583-template-zip")
            .with_title("ISO 8583:1987 ASCII Profile 1.0.0")
            .with_description(
                "The exact application-owned ZIP template. MCP returns base64 blob contents.",
            )
            .with_mime_type("application/zip"),
    ]
}

pub fn text(uri: &str) -> Option<(&'static str, &'static str)> {
    match uri {
        AUTHORING_GUIDE_URI => Some(("text/markdown", RULES_AND_PROTOCOL_PACKAGES)),
        HOST_API_URI => Some(("text/markdown", HOST_API)),
        SOCKET_AUTHORING_URI => Some(("text/markdown", SOCKET_AUTHORING)),
        CERTIFICATE_CONCEPTS_URI => Some(("text/markdown", CERTIFICATE_CONCEPTS)),
        APP_INTEGRATION_GUIDE_URI => Some(("text/markdown", APP_INTEGRATION_GUIDE)),
        EXTERNAL_PACKAGE_INTEGRATION_GUIDE_URI => {
            Some(("text/markdown", EXTERNAL_PACKAGE_INTEGRATION_GUIDE))
        }
        DIAGNOSTIC_ARCHITECTURE_URI => Some(("text/markdown", DIAGNOSTIC_ARCHITECTURE)),
        TOOL_REFERENCE_URI => Some(("text/markdown", TOOL_REFERENCE)),
        VALIDATION_PLAYBOOK_URI => Some(("text/markdown", VALIDATION_PLAYBOOK)),
        ISO8583_MANIFEST_URI => Some(("application/toml", ISO8583_MANIFEST)),
        ISO8583_SCHEMA_URI => Some(("application/toml", ISO8583_SCHEMA)),
        ISO8583_PROTOCOL_SOURCE_URI => Some(("text/x-rhai", ISO8583_PROTOCOL_SOURCE)),
        ISO8583_DISPLAY_SOURCE_URI => Some(("text/x-rhai", ISO8583_DISPLAY_SOURCE)),
        ISO8583_LIBRARY_SOURCE_URI => Some(("text/x-rhai", ISO8583_LIBRARY_SOURCE)),
        _ => None,
    }
}
