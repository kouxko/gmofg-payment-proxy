use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnvironmentMaterials {
    certificates: Vec<CertificateMaterialInput>,
    secrets: Vec<SecretMaterialInput>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CertificateMaterialInput {
    alias: String,
    role: CertificateMaterialRole,
    encoding: CertificateMaterialEncoding,
    content: String,
    #[serde(deserialize_with = "required_nullable")]
    password: Option<String>,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SecretMaterialInput {
    alias: String,
    role: SecretMaterialRole,
    username: String,
    password: String,
    label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SecretMaterialRole {
    ProxyBasicAuth,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
