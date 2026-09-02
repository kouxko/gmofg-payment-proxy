//! Application-owned Revision 16 technical-validation orchestration.

use std::{sync::Arc, time::Duration};

use super::{EnvironmentCandidateId, EnvironmentStatusCode, materials::EnvironmentMaterials};
use crate::AppResult;
use async_trait::async_trait;
use intercept_proxy_domain::{ProtocolPackageRef, ProxyWorkspace};
use serde::{Deserialize, Serialize};
mod outcome;
mod projection;
mod runner;
mod status_code;

const ORDER: [EnvironmentValidationLayer; 7] = [
    EnvironmentValidationLayer::Schema,
    EnvironmentValidationLayer::Domain,
    EnvironmentValidationLayer::Material,
    EnvironmentValidationLayer::PackageProjection,
    EnvironmentValidationLayer::DnsTcpPort,
    EnvironmentValidationLayer::TlsMtls,
    EnvironmentValidationLayer::PreviewBaseline,
];

const BUDGETS: [Duration; 7] = [
    Duration::from_secs(1),
    Duration::from_secs(1),
    Duration::from_secs(6),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(10),
    Duration::from_secs(2),
];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentValidationLayer {
    Schema,
    Domain,
    Material,
    PackageProjection,
    DnsTcpPort,
    TlsMtls,
    PreviewBaseline,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentValidationStatus {
    Passed,
    Failed,
    Cancelled,
    NotApplicable,
    SkippedDependency,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentValidationResult {
    layer: EnvironmentValidationLayer,
    status: EnvironmentValidationStatus,
    code: Option<EnvironmentStatusCode>,
    reason: Option<&'static str>,
    duration_ms: u64,
}

impl EnvironmentValidationResult {
    pub const fn layer(&self) -> EnvironmentValidationLayer {
        self.layer
    }

    pub const fn status(&self) -> EnvironmentValidationStatus {
        self.status
    }

    pub const fn code(&self) -> Option<EnvironmentStatusCode> {
        self.code
    }

    pub const fn reason(&self) -> Option<&'static str> {
        self.reason
    }

    pub const fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentValidationReport {
    layers: Vec<EnvironmentValidationResult>,
    status_code: Option<EnvironmentStatusCode>,
}

impl EnvironmentValidationReport {
    pub fn layers(&self) -> &[EnvironmentValidationResult] {
        &self.layers
    }

    pub const fn status_code(&self) -> Option<EnvironmentStatusCode> {
        self.status_code
    }

    pub(crate) fn into_layers(self) -> Vec<EnvironmentValidationResult> {
        self.layers
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentDnsTcpTarget {
    host: String,
    port: u16,
}

impl EnvironmentDnsTcpTarget {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub const fn port(&self) -> u16 {
        self.port
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentTlsMtlsTarget {
    host: String,
    port: u16,
    server_name: Option<String>,
    upstream_ca_alias: Option<String>,
    client_identity_alias: Option<String>,
    verify_hostname: bool,
}

impl EnvironmentTlsMtlsTarget {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn server_name(&self) -> Option<&str> {
        self.server_name.as_deref()
    }

    pub fn upstream_ca_alias(&self) -> Option<&str> {
        self.upstream_ca_alias.as_deref()
    }

    pub fn client_identity_alias(&self) -> Option<&str> {
        self.client_identity_alias.as_deref()
    }

    pub const fn verify_hostname(&self) -> bool {
        self.verify_hostname
    }
}

#[derive(Debug)]
pub struct EnvironmentValidationLayerRequest<'a> {
    layer: EnvironmentValidationLayer,
    exact_package_refs: &'a [ProtocolPackageRef],
    dns_tcp_targets: &'a [EnvironmentDnsTcpTarget],
    tls_mtls_targets: &'a [EnvironmentTlsMtlsTarget],
    installation_root_selectors: &'a [String],
    materials: Option<&'a EnvironmentMaterials>,
}

impl EnvironmentValidationLayerRequest<'_> {
    fn empty(layer: EnvironmentValidationLayer) -> Self {
        Self {
            layer,
            exact_package_refs: &[],
            dns_tcp_targets: &[],
            tls_mtls_targets: &[],
            installation_root_selectors: &[],
            materials: None,
        }
    }

    pub const fn layer(&self) -> EnvironmentValidationLayer {
        self.layer
    }

    pub fn exact_package_refs(&self) -> &[ProtocolPackageRef] {
        self.exact_package_refs
    }

    pub fn dns_tcp_targets(&self) -> &[EnvironmentDnsTcpTarget] {
        self.dns_tcp_targets
    }

    pub fn tls_mtls_targets(&self) -> &[EnvironmentTlsMtlsTarget] {
        self.tls_mtls_targets
    }

    pub const fn installation_root_selector(&self) -> Option<&str> {
        match self.installation_root_selectors {
            [selector] => Some(selector.as_str()),
            _ => None,
        }
    }

    pub fn installation_root_selectors(&self) -> impl Iterator<Item = &str> {
        self.installation_root_selectors.iter().map(String::as_str)
    }

    pub fn visit_materials(&self, mut visitor: impl FnMut(EnvironmentMaterialProbe<'_>)) {
        let Some(materials) = self.materials else {
            return;
        };
        for certificate in &materials.certificates {
            visitor(EnvironmentMaterialProbe {
                kind: EnvironmentMaterialProbeKind::Certificate,
                alias: &certificate.alias,
                role: certificate.role(),
                encoding: Some(certificate.encoding()),
                content: certificate.content(),
                password: certificate.password(),
                username: None,
            });
        }
        for secret in &materials.secrets {
            visitor(EnvironmentMaterialProbe {
                kind: EnvironmentMaterialProbeKind::Secret,
                alias: &secret.alias,
                role: secret.role(),
                encoding: None,
                content: secret.password(),
                password: None,
                username: Some(secret.username()),
            });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentMaterialProbeKind {
    Certificate,
    Secret,
}

#[derive(Clone, Copy, Debug)]
pub struct EnvironmentMaterialProbe<'a> {
    kind: EnvironmentMaterialProbeKind,
    alias: &'a str,
    role: &'a str,
    encoding: Option<&'a str>,
    content: &'a str,
    password: Option<&'a str>,
    username: Option<&'a str>,
}

impl EnvironmentMaterialProbe<'_> {
    pub const fn kind(&self) -> EnvironmentMaterialProbeKind {
        self.kind
    }
    pub const fn alias(&self) -> &str {
        self.alias
    }
    pub const fn role(&self) -> &str {
        self.role
    }
    pub const fn encoding(&self) -> Option<&str> {
        self.encoding
    }
    pub const fn content(&self) -> &str {
        self.content
    }
    pub const fn password(&self) -> Option<&str> {
        self.password
    }
    pub const fn username(&self) -> Option<&str> {
        self.username
    }
}

#[async_trait]
pub trait EnvironmentValidationLayerPort: Send + Sync {
    async fn validate_layer(
        &self,
        request: EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus>;
}

pub(crate) struct EnvironmentPreviewBaselineRequest<'a> {
    candidate_id: &'a EnvironmentCandidateId,
    #[cfg(test)]
    validated_candidate_json: &'a [u8],
    prior_layers: &'a [EnvironmentValidationResult],
    projected_candidate: Option<&'a EnvironmentProjectedCandidate>,
}

impl EnvironmentPreviewBaselineRequest<'_> {
    pub(crate) const fn candidate_id(&self) -> &EnvironmentCandidateId {
        self.candidate_id
    }

    #[cfg(test)]
    pub(crate) const fn validated_candidate_json(&self) -> &[u8] {
        self.validated_candidate_json
    }

    pub(crate) const fn prior_layers(&self) -> &[EnvironmentValidationResult] {
        self.prior_layers
    }

    pub(crate) const fn projected_candidate(&self) -> Option<&EnvironmentProjectedCandidate> {
        self.projected_candidate
    }
}

pub(crate) struct EnvironmentProjectedCandidate {
    validation: projection::ValidationProjection,
    workspace: ProxyWorkspace,
}

impl EnvironmentProjectedCandidate {
    pub(crate) fn project(
        candidate: super::EnvironmentConfigurationCandidateV1,
        persisted_workspace: Option<&ProxyWorkspace>,
        allocator: &dyn super::identity::EnvironmentIdentityAllocatorPort,
    ) -> AppResult<Self> {
        Self::project_with_workspace_scope_and_checkpoint(
            candidate,
            persisted_workspace,
            &[],
            allocator,
            &NoopEnvironmentValidationCheckpoint,
        )
    }

    pub(crate) fn project_with_workspace_scope_and_checkpoint(
        candidate: super::EnvironmentConfigurationCandidateV1,
        persisted_workspace: Option<&ProxyWorkspace>,
        workspace_scope: &[ProxyWorkspace],
        allocator: &dyn super::identity::EnvironmentIdentityAllocatorPort,
        checkpoint: &dyn EnvironmentValidationCheckpoint,
    ) -> AppResult<Self> {
        let validation =
            projection::ValidationProjection::project_with_checkpoint(candidate, checkpoint)?;
        let workspace = super::project_candidate_workspace(
            validation.candidate(),
            persisted_workspace,
            workspace_scope,
            allocator,
            checkpoint,
        )?;
        Ok(Self {
            validation,
            workspace,
        })
    }

    pub(crate) const fn candidate(&self) -> &super::EnvironmentConfigurationCandidateV1 {
        self.validation.candidate()
    }

    pub(crate) const fn workspace(&self) -> &ProxyWorkspace {
        &self.workspace
    }

    fn request(&self, layer: EnvironmentValidationLayer) -> EnvironmentValidationLayerRequest<'_> {
        self.validation.request(layer)
    }
}

#[async_trait]
pub(crate) trait EnvironmentDomainProjectionPort: Send + Sync {
    async fn project_environment_candidate(
        &self,
        candidate: super::EnvironmentConfigurationCandidateV1,
        checkpoint: &dyn EnvironmentValidationCheckpoint,
    ) -> AppResult<EnvironmentProjectedCandidate>;
}

pub(crate) trait EnvironmentValidationCheckpoint: Send + Sync {
    fn checkpoint(&self) -> bool;
}

struct NoopEnvironmentValidationCheckpoint;

impl EnvironmentValidationCheckpoint for NoopEnvironmentValidationCheckpoint {
    fn checkpoint(&self) -> bool {
        false
    }
}

#[async_trait]
pub(crate) trait EnvironmentPreviewBaselinePort: Send + Sync {
    fn domain_projection_port(&self) -> Option<&dyn EnvironmentDomainProjectionPort> {
        None
    }

    async fn validate_preview_baseline(
        &self,
        request: EnvironmentPreviewBaselineRequest<'_>,
    ) -> AppResult<()>;
}

pub struct EnvironmentCandidateValidator<P: ?Sized> {
    port: Arc<P>,
    total_deadline: Duration,
    layer_budgets: [Duration; 7],
    cpu_work_probe: Option<Arc<dyn EnvironmentCpuWorkProbe>>,
}

pub(crate) trait EnvironmentCpuWorkProbe: Send + Sync {
    fn checkpoint(&self, _layer: EnvironmentValidationLayer, _checkpoint_index: usize) {}
    fn candidate_buffer_dropped(&self) {}
}

impl<P: ?Sized> std::fmt::Debug for EnvironmentCandidateValidator<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentCandidateValidator")
            .field("total_deadline", &self.total_deadline)
            .field("layer_budgets", &self.layer_budgets)
            .finish_non_exhaustive()
    }
}

impl<P> EnvironmentCandidateValidator<P>
where
    P: EnvironmentValidationLayerPort + ?Sized,
{
    pub fn new(port: Arc<P>) -> Self {
        Self {
            port,
            total_deadline: Duration::from_secs(30),
            layer_budgets: BUDGETS,
            cpu_work_probe: None,
        }
    }

    #[must_use]
    pub const fn with_total_deadline(mut self, total_deadline: Duration) -> Self {
        self.total_deadline = total_deadline;
        self
    }

    #[must_use]
    pub fn with_layer_budget(
        mut self,
        layer: EnvironmentValidationLayer,
        budget: Duration,
    ) -> Self {
        let index = ORDER
            .iter()
            .position(|candidate| *candidate == layer)
            .expect("validation layer belongs to the fixed Revision 16 order");
        self.layer_budgets[index] = budget;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_cpu_work_probe(mut self, probe: Arc<dyn EnvironmentCpuWorkProbe>) -> Self {
        self.cpu_work_probe = Some(probe);
        self
    }
}
