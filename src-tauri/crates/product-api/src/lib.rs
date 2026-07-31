//! Product-specific policy boundary for the otherwise reusable proxy core.
//!
//! This crate intentionally contains contracts only. Concrete products own
//! their names, bundled trust anchors, and any explicitly enabled test-only
//! signing material.

use std::{collections::BTreeSet, error::Error, fmt, net::IpAddr, sync::Arc};

/// Product-owned listener/upstream definition.
///
/// The reusable core treats channel identifiers as opaque values. A concrete
/// product supplies the catalog, labels, ports, and upstream defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductChannel {
    pub id: &'static str,
    pub display_name: &'static str,
    pub enabled_by_default: bool,
    pub listen_port: u16,
    pub upstream_url: &'static str,
}

/// Product-owned persistence and secret-protection namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductStorageNamespace {
    pub database_file_name: &'static str,
    pub secret_service: &'static str,
    pub secret_account: &'static str,
    pub secret_envelope_magic: &'static [u8; 5],
    pub secret_aad: &'static [u8],
}

/// Product-owned aliases for settings written by an older product release.
///
/// Generic persistence code understands how to migrate a channel catalog, but
/// it must not know the legacy field names chosen by a concrete product.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacySettingsChannelMapping {
    pub channel_id: &'static str,
    pub enabled_field: &'static str,
    pub port_field: &'static str,
    pub upstream_url_field: &'static str,
}

/// Product-owned compatibility metadata for persisted data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProductPersistenceMigrations {
    pub settings_channels: &'static [LegacySettingsChannelMapping],
    pub terminal_body_fields: &'static [&'static str],
}

/// Product-facing terminology consumed by generic adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductLabels {
    pub client_name: &'static str,
    pub upstream_name: &'static str,
    pub fault_rule_name_prefix: &'static str,
}

/// Product-selected presentation metadata for a generic fault capability.
///
/// The `id` names a capability implemented by the generic rule engine. The
/// product decides which capabilities are exposed and how they are described.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductFaultTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub stage_text: &'static str,
    pub behavior_text: &'static str,
    pub affected_party_text: &'static str,
    pub default_channel_id: &'static str,
    pub risk_text: &'static str,
}

pub const STANDARD_FAULT_CAPABILITY_IDS: &[&str] = &[
    "reject_tls_handshake",
    "disconnect_before_upstream",
    "request_delay",
    "modify_request_json",
    "drop_upstream_response",
    "upstream_connect_timeout",
    "upstream_write_timeout",
    "upstream_read_timeout",
    "response_delay",
    "custom_http_status",
    "mock_json",
    "invalid_json",
    "wrong_content_length",
    "truncate_response",
    "throttle_upstream",
    "throttle_downstream",
    "jitter_upstream",
    "jitter_downstream",
    "intermittent_upstream",
    "intermittent_downstream",
    "disconnect_upstream_mid_body",
    "disconnect_downstream_mid_body",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassifiedRequest {
    pub request_id: Option<String>,
    pub request_type: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProductHeader<'a> {
    pub name: &'a [u8],
    pub value: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub struct ProductMessageContext<'a> {
    pub channel_id: &'a str,
    pub start_line: &'a [u8],
    pub headers: &'a [ProductHeader<'a>],
    pub body: &'a [u8],
}

/// Product-specific request metadata extraction.
///
/// Generic transport and storage code never knows product JSON field names.
pub trait RequestClassifier: fmt::Debug + Send + Sync {
    fn classify(&self, message: ProductMessageContext<'_>) -> ClassifiedRequest;
}

/// Stable product-boundary error returned by codecs and future product hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductError {
    pub code: &'static str,
    pub message: String,
}

impl ProductError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ProductError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for ProductError {}

/// Product-selected body encoding contract.
pub trait BodyCodec: fmt::Debug + Send + Sync {
    fn id(&self) -> &'static str;

    fn name(&self) -> &'static str;

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError>;

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError>;
}

/// Static certificate authority material used only by an explicitly enabled
/// isolated-test product profile.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedTestCertificateAuthority {
    pub public_certificate_pem: &'static [u8],
    pub signing_key_pem: &'static str,
    pub required_subject_marker: &'static str,
}

/// Product-owned text shown by the generic certificate adapter.
#[derive(Debug, Clone, Copy)]
pub struct CertificateLabels {
    pub root_name: &'static str,
    pub root_usage: &'static str,
    pub leaf_name: &'static str,
    pub leaf_usage: &'static str,
    pub client_identity_name: &'static str,
    pub client_identity_usage: &'static str,
    pub upstream_name: &'static str,
    pub upstream_bundled_usage: &'static str,
    pub upstream_override_usage: &'static str,
    pub ready_status: &'static str,
    pub incomplete_status: &'static str,
    pub already_exists_message: &'static str,
    pub export_cancelled_message: &'static str,
    pub export_success_message: &'static str,
}

/// Certificate assets and behavior selected by the outer product composition.
///
/// `embedded_test_authority` must return `None` unless the concrete product was
/// constructed with an explicit test-only opt-in. This makes private signing
/// material fail closed rather than silently becoming generic infrastructure.
pub trait ProductCertificatePolicy: fmt::Debug + Send + Sync {
    fn public_root_ca_pem(&self) -> &'static [u8];

    fn embedded_test_authority(&self) -> Option<EmbeddedTestCertificateAuthority>;

    fn bundled_upstream_ca_pem(&self) -> Option<&'static [u8]>;

    fn labels(&self) -> CertificateLabels;
}

/// Product profile injected into the UI-neutral Rust host.
pub trait ProductProfile: fmt::Debug + Send + Sync {
    fn id(&self) -> &'static str;

    fn name(&self) -> &'static str;

    fn channels(&self) -> &'static [ProductChannel];

    fn storage(&self) -> ProductStorageNamespace;

    fn persistence_migrations(&self) -> ProductPersistenceMigrations {
        ProductPersistenceMigrations::default()
    }

    fn labels(&self) -> ProductLabels;

    fn fault_templates(&self) -> &'static [ProductFaultTemplate];

    fn request_classifier(&self) -> Arc<dyn RequestClassifier>;

    fn certificates(&self) -> &dyn ProductCertificatePolicy;

    fn body_codec(&self) -> Arc<dyn BodyCodec>;
}

/// Validates the static product contract before the host opens storage or
/// starts background tasks.
pub fn validate_product_profile(product: &dyn ProductProfile) -> Result<(), ProductError> {
    let channels = product.channels();
    if channels.is_empty() {
        return Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            "product must declare at least one channel",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut enabled_ports = BTreeSet::new();
    for channel in channels {
        validate_channel_id(channel.id)?;
        if !ids.insert(channel.id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!("duplicate product channel ID {:?}", channel.id),
            ));
        }
        if channel.display_name.trim().is_empty() {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!("channel {:?} has an empty display name", channel.id),
            ));
        }
        if channel.enabled_by_default
            && (channel.listen_port == 0 || !enabled_ports.insert(channel.listen_port))
        {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "enabled channel {:?} has a zero or duplicate listen port",
                    channel.id
                ),
            ));
        }
        if !valid_https_url(channel.upstream_url) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!("channel {:?} has an invalid HTTPS upstream URL", channel.id),
            ));
        }
    }

    validate_product_storage(product.storage())?;
    validate_persistence_migrations(product.persistence_migrations(), &ids)?;
    let mut template_ids = BTreeSet::new();
    for template in product.fault_templates() {
        if template.id.is_empty() || !template_ids.insert(template.id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!("fault template ID {:?} is empty or duplicated", template.id),
            ));
        }
        if !STANDARD_FAULT_CAPABILITY_IDS.contains(&template.id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "fault template {:?} names an unknown capability",
                    template.id
                ),
            ));
        }
        if !ids.contains(template.default_channel_id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "fault template {:?} references unknown channel {:?}",
                    template.id, template.default_channel_id
                ),
            ));
        }
    }
    validate_product_labels(product.labels())
}

fn validate_persistence_migrations(
    migrations: ProductPersistenceMigrations,
    channel_ids: &BTreeSet<&str>,
) -> Result<(), ProductError> {
    let mut settings_fields = BTreeSet::new();
    for mapping in migrations.settings_channels {
        if !channel_ids.contains(mapping.channel_id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "legacy settings mapping references unknown channel {:?}",
                    mapping.channel_id
                ),
            ));
        }
        for field in [
            mapping.enabled_field,
            mapping.port_field,
            mapping.upstream_url_field,
        ] {
            if field.trim().is_empty() || !settings_fields.insert(field) {
                return Err(ProductError::new(
                    "PRODUCT_PROFILE_INVALID",
                    "legacy settings field names must be non-empty and unique",
                ));
            }
        }
    }
    let mut terminal_fields = BTreeSet::new();
    for field in migrations.terminal_body_fields {
        if field.trim().is_empty() || !terminal_fields.insert(*field) || *field == "body_bytes" {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                "legacy terminal body fields must be non-empty, unique aliases",
            ));
        }
    }
    Ok(())
}

fn validate_product_storage(storage: ProductStorageNamespace) -> Result<(), ProductError> {
    if [
        storage.database_file_name,
        storage.secret_service,
        storage.secret_account,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
        || storage.secret_aad.is_empty()
    {
        return Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            "product storage namespace must be non-empty",
        ));
    }
    if !valid_database_file_name(storage.database_file_name) {
        return Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            "product database file name must be one portable file-name component",
        ));
    }
    Ok(())
}

fn validate_product_labels(labels: ProductLabels) -> Result<(), ProductError> {
    if [
        labels.client_name,
        labels.upstream_name,
        labels.fault_rule_name_prefix,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            "product labels must be non-empty",
        ));
    }
    Ok(())
}

fn validate_channel_id(value: &str) -> Result<(), ProductError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    if valid {
        Ok(())
    } else {
        Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            format!("invalid product channel ID {value:?}"),
        ))
    }
}

fn valid_database_file_name(value: &str) -> bool {
    !matches!(value, "." | "..")
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b':' | 0))
}

fn valid_https_url(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    let authority = rest
        .split_once(['/', '?', '#'])
        .map_or(rest, |(authority, _)| authority);
    if authority.is_empty() || authority.contains('@') || authority.chars().any(char::is_whitespace)
    {
        return false;
    }
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(end) = bracketed.find(']') else {
            return false;
        };
        let host = &bracketed[..end];
        let suffix = &bracketed[end + 1..];
        if !valid_optional_port(suffix) {
            return false;
        }
        return host
            .parse::<IpAddr>()
            .is_ok_and(|address| matches!(address, IpAddr::V6(_)));
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') || !valid_port(port) {
            return false;
        }
        host
    } else {
        authority
    };
    host.parse::<IpAddr>().is_ok()
        || (!host.is_empty()
            && host.split('.').all(|label| {
                !label.is_empty()
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }))
}

fn valid_optional_port(value: &str) -> bool {
    value.is_empty() || value.strip_prefix(':').is_some_and(valid_port)
}

fn valid_port(value: &str) -> bool {
    value.parse::<u16>().is_ok_and(|port| port > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestProfile {
        channels: &'static [ProductChannel],
        storage: ProductStorageNamespace,
        faults: &'static [ProductFaultTemplate],
    }

    #[derive(Debug)]
    struct Utf8Codec;

    #[derive(Debug)]
    struct EmptyClassifier;

    const VALID_CHANNELS: &[ProductChannel] = &[
        channel("alpha_2.v1", 20_001, "https://alpha.example.test"),
        channel("A-channel", 20_002, "https://beta.example.test"),
    ];
    const DUPLICATE_PORTS: &[ProductChannel] = &[
        channel("alpha", 20_001, "https://alpha.example.test"),
        channel("beta", 20_001, "https://beta.example.test"),
    ];
    const INVALID_ID: &[ProductChannel] =
        &[channel("-alpha", 20_001, "https://alpha.example.test")];
    const INVALID_URL: &[ProductChannel] = &[channel("alpha", 20_001, "https://:bad")];
    const VALID_FAULTS: &[ProductFaultTemplate] = &[fault("request_delay", "alpha_2.v1")];
    const UNKNOWN_CHANNEL_FAULTS: &[ProductFaultTemplate] = &[fault("request_delay", "missing")];
    const DUPLICATE_FAULTS: &[ProductFaultTemplate] = &[
        fault("request_delay", "alpha_2.v1"),
        fault("request_delay", "A-channel"),
    ];
    const UNKNOWN_CAPABILITY: &[ProductFaultTemplate] = &[fault("not-supported", "alpha_2.v1")];

    const fn channel(id: &'static str, port: u16, url: &'static str) -> ProductChannel {
        ProductChannel {
            id,
            display_name: id,
            enabled_by_default: true,
            listen_port: port,
            upstream_url: url,
        }
    }

    const fn fault(id: &'static str, channel: &'static str) -> ProductFaultTemplate {
        ProductFaultTemplate {
            id,
            name: id,
            stage_text: "request",
            behavior_text: "delay",
            affected_party_text: "client",
            default_channel_id: channel,
            risk_text: "low",
        }
    }

    const fn storage() -> ProductStorageNamespace {
        ProductStorageNamespace {
            database_file_name: "test.sqlite3",
            secret_service: "com.example.test",
            secret_account: "key",
            secret_envelope_magic: b"TSTK1",
            secret_aad: b"test/aad",
        }
    }

    impl BodyCodec for Utf8Codec {
        fn id(&self) -> &'static str {
            "utf-8"
        }

        fn name(&self) -> &'static str {
            "UTF-8"
        }

        fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
            String::from_utf8(bytes.to_vec())
                .map_err(|error| ProductError::new("DECODE", error.to_string()))
        }

        fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
            Ok(text.as_bytes().to_vec())
        }
    }

    impl RequestClassifier for EmptyClassifier {
        fn classify(&self, _: ProductMessageContext<'_>) -> ClassifiedRequest {
            ClassifiedRequest::default()
        }
    }

    impl ProductCertificatePolicy for TestProfile {
        fn public_root_ca_pem(&self) -> &'static [u8] {
            b""
        }

        fn embedded_test_authority(&self) -> Option<EmbeddedTestCertificateAuthority> {
            None
        }

        fn bundled_upstream_ca_pem(&self) -> Option<&'static [u8]> {
            None
        }

        fn labels(&self) -> CertificateLabels {
            CertificateLabels {
                root_name: "root",
                root_usage: "test",
                leaf_name: "leaf",
                leaf_usage: "test",
                client_identity_name: "identity",
                client_identity_usage: "test",
                upstream_name: "upstream",
                upstream_bundled_usage: "test",
                upstream_override_usage: "test",
                ready_status: "ready",
                incomplete_status: "incomplete",
                already_exists_message: "exists",
                export_cancelled_message: "cancelled",
                export_success_message: "exported",
            }
        }
    }

    impl ProductProfile for TestProfile {
        fn id(&self) -> &'static str {
            "test"
        }

        fn name(&self) -> &'static str {
            "Test"
        }

        fn channels(&self) -> &'static [ProductChannel] {
            self.channels
        }

        fn storage(&self) -> ProductStorageNamespace {
            self.storage
        }

        fn labels(&self) -> ProductLabels {
            ProductLabels {
                client_name: "Client",
                upstream_name: "Upstream",
                fault_rule_name_prefix: "Fault · ",
            }
        }

        fn fault_templates(&self) -> &'static [ProductFaultTemplate] {
            self.faults
        }

        fn request_classifier(&self) -> Arc<dyn RequestClassifier> {
            Arc::new(EmptyClassifier)
        }

        fn certificates(&self) -> &dyn ProductCertificatePolicy {
            self
        }

        fn body_codec(&self) -> Arc<dyn BodyCodec> {
            Arc::new(Utf8Codec)
        }
    }

    #[test]
    fn profile_validation_accepts_runtime_channel_id_grammar() {
        validate_product_profile(&TestProfile {
            channels: VALID_CHANNELS,
            storage: storage(),
            faults: VALID_FAULTS,
        })
        .unwrap();
    }

    #[test]
    fn profile_validation_rejects_every_cross_boundary_invariant() {
        let mut empty_storage = storage();
        empty_storage.secret_service = "";
        for profile in [
            TestProfile {
                channels: &[],
                storage: storage(),
                faults: &[],
            },
            TestProfile {
                channels: INVALID_ID,
                storage: storage(),
                faults: &[],
            },
            TestProfile {
                channels: DUPLICATE_PORTS,
                storage: storage(),
                faults: &[],
            },
            TestProfile {
                channels: INVALID_URL,
                storage: storage(),
                faults: &[],
            },
            TestProfile {
                channels: VALID_CHANNELS,
                storage: empty_storage,
                faults: VALID_FAULTS,
            },
            TestProfile {
                channels: VALID_CHANNELS,
                storage: storage(),
                faults: UNKNOWN_CHANNEL_FAULTS,
            },
            TestProfile {
                channels: VALID_CHANNELS,
                storage: storage(),
                faults: DUPLICATE_FAULTS,
            },
            TestProfile {
                channels: VALID_CHANNELS,
                storage: storage(),
                faults: UNKNOWN_CAPABILITY,
            },
        ] {
            assert_eq!(
                validate_product_profile(&profile).unwrap_err().code,
                "PRODUCT_PROFILE_INVALID"
            );
        }
    }

    #[test]
    fn profile_validation_rejects_database_paths_outside_the_product_directory() {
        for database_file_name in [
            "../escape.sqlite3",
            "/tmp/escape.sqlite3",
            r"..\escape.sqlite3",
            r"C:\escape.sqlite3",
            ".",
            "..",
        ] {
            let mut invalid_storage = storage();
            invalid_storage.database_file_name = database_file_name;
            let error = validate_product_profile(&TestProfile {
                channels: VALID_CHANNELS,
                storage: invalid_storage,
                faults: VALID_FAULTS,
            })
            .expect_err("database path must remain inside the product data directory");
            assert_eq!(error.code, "PRODUCT_PROFILE_INVALID");
        }
    }
}
