//! Stable, read-only resources exposed by the MCP adapter.

use rmcp::model::Resource;

pub const AUTHORING_GUIDE_URI: &str = "intercept-proxy://docs/protocol-package-authoring/1.0";
pub const HOST_API_URI: &str = "intercept-proxy://docs/protocol-package-host-api/1.0";
pub const SOCKET_AUTHORING_URI: &str =
    "intercept-proxy://docs/socket-protocol-package-authoring/1.0";
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

const PROTOCOL_BOUNDARIES: &str = include_str!("../../../docs/architecture/protocol-boundaries.md");
const HOST_API: &str = include_str!("../../../templates/socket-protocol/API.md");
const SOCKET_AUTHORING: &str = include_str!("../../../templates/socket-protocol/AUTHORING.md");
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
            .with_title("Intercept Proxy 1.0 protocol package boundaries")
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
        AUTHORING_GUIDE_URI => Some(("text/markdown", PROTOCOL_BOUNDARIES)),
        HOST_API_URI => Some(("text/markdown", HOST_API)),
        SOCKET_AUTHORING_URI => Some(("text/markdown", SOCKET_AUTHORING)),
        ISO8583_MANIFEST_URI => Some(("application/toml", ISO8583_MANIFEST)),
        ISO8583_SCHEMA_URI => Some(("application/toml", ISO8583_SCHEMA)),
        ISO8583_PROTOCOL_SOURCE_URI => Some(("text/x-rhai", ISO8583_PROTOCOL_SOURCE)),
        ISO8583_DISPLAY_SOURCE_URI => Some(("text/x-rhai", ISO8583_DISPLAY_SOURCE)),
        ISO8583_LIBRARY_SOURCE_URI => Some(("text/x-rhai", ISO8583_LIBRARY_SOURCE)),
        _ => None,
    }
}
