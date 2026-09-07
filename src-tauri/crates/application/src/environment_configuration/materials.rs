use std::fmt;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{AppError, AppResult};

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnvironmentMaterials {
    pub(super) certificates: Vec<CertificateMaterialInput>,
    pub(super) secrets: Vec<SecretMaterialInput>,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CertificateMaterialInput {
    pub(super) alias: String,
    role: CertificateMaterialRole,
    encoding: CertificateMaterialEncoding,
    content: SensitiveString,
    #[serde(deserialize_with = "required_nullable")]
    password: Option<SensitiveString>,
    label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CertificateMaterialRole {
    DownstreamServerIdentity,
    DownstreamClientTrust,
    UpstreamClientIdentity,
    UpstreamServerTrust,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CertificateMaterialEncoding {
    Pem,
    Base64Der,
    Pkcs12Base64,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SecretMaterialInput {
    pub(super) alias: String,
    role: SecretMaterialRole,
    username: SensitiveString,
    password: SensitiveString,
    label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SecretMaterialRole {
    ProxyBasicAuth,
}

struct SensitiveString(Zeroizing<String>);

impl CertificateMaterialInput {
    pub(super) fn role(&self) -> &'static str {
        match self.role {
            CertificateMaterialRole::DownstreamServerIdentity => "downstream_server_identity",
            CertificateMaterialRole::DownstreamClientTrust => "downstream_client_trust",
            CertificateMaterialRole::UpstreamClientIdentity => "upstream_client_identity",
            CertificateMaterialRole::UpstreamServerTrust => "upstream_server_trust",
        }
    }

    pub(super) fn encoding(&self) -> &'static str {
        match self.encoding {
            CertificateMaterialEncoding::Pem => "pem",
            CertificateMaterialEncoding::Base64Der => "base64_der",
            CertificateMaterialEncoding::Pkcs12Base64 => "pkcs12_base64",
        }
    }

    pub(super) fn content(&self) -> &str {
        &self.content.0
    }

    pub(super) fn password(&self) -> Option<&str> {
        self.password.as_ref().map(|password| password.0.as_str())
    }
}

impl EnvironmentMaterials {
    pub(super) fn certificate_aliases(&self) -> Vec<&str> {
        self.certificates
            .iter()
            .map(|material| material.alias.as_str())
            .collect()
    }

    pub(super) fn secret_aliases(&self) -> Vec<&str> {
        self.secrets
            .iter()
            .map(|material| material.alias.as_str())
            .collect()
    }

    pub(super) fn public_certificates(&self) -> Vec<serde_json::Value> {
        self.certificates
            .iter()
            .map(|material| {
                serde_json::json!({
                    "alias": material.alias,
                    "role": material.role(),
                    "encoding": material.encoding(),
                    "label": material.label,
                })
            })
            .collect()
    }

    pub(super) fn public_secrets(&self) -> Vec<serde_json::Value> {
        self.secrets
            .iter()
            .map(|material| {
                serde_json::json!({
                    "alias": material.alias,
                    "role": material.role(),
                    "label": material.label,
                })
            })
            .collect()
    }

    pub(super) fn certificate_roles(&self) -> impl Iterator<Item = (&str, &str)> {
        self.certificates
            .iter()
            .map(|material| (material.alias.as_str(), material.role()))
    }

    pub(super) fn secret_roles(&self) -> impl Iterator<Item = (&str, &str)> {
        self.secrets
            .iter()
            .map(|material| (material.alias.as_str(), material.role()))
    }

    pub(super) fn validate_domain_limits(&self) -> AppResult<()> {
        for certificate in &self.certificates {
            let decoded_len = match certificate.encoding {
                CertificateMaterialEncoding::Pem => certificate.content.0.len(),
                CertificateMaterialEncoding::Base64Der
                | CertificateMaterialEncoding::Pkcs12Base64 => STANDARD
                    .decode(certificate.content.0.as_bytes())
                    .map(Zeroizing::new)
                    .map_or(0, |decoded| decoded.len()),
            };
            if !valid_alias(&certificate.alias)
                || decoded_len > 256 * 1024
                || certificate
                    .password
                    .as_ref()
                    .is_some_and(|password| password.0.len() > 4 * 1024)
            {
                return Err(dto_limit_error());
            }
        }
        if self
            .secrets
            .iter()
            .any(|secret| !valid_alias(&secret.alias) || secret.password.0.len() > 4 * 1024)
        {
            return Err(dto_limit_error());
        }
        Ok(())
    }
}

fn valid_alias(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn dto_limit_error() -> AppError {
    AppError::new(
        "DTO_LIMIT_EXCEEDED",
        "environment candidate exceeds its DTO limit",
    )
}

impl SecretMaterialInput {
    pub(super) const fn role(&self) -> &'static str {
        match self.role {
            SecretMaterialRole::ProxyBasicAuth => "proxy_basic_auth",
        }
    }
    pub(super) fn username(&self) -> &str {
        &self.username.0
    }

    pub(super) fn password(&self) -> &str {
        &self.password.0
    }
}

impl<'de> Deserialize<'de> for SensitiveString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .map(Zeroizing::new)
            .map(Self)
    }
}

impl Serialize for SensitiveString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl PartialEq for SensitiveString {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl fmt::Debug for SensitiveString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveString([REDACTED])")
    }
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
