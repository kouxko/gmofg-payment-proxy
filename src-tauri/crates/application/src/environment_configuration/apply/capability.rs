use std::collections::BTreeMap;

use zeroize::Zeroizing;

use crate::{AppResult, ProxyWorkspace};

use super::{EnvironmentApplyGenerations, EnvironmentCommitTarget, EnvironmentSelectionPolicy};

/// Infrastructure-owned capability consumed only through the commit visitor. Implementations
/// expose no arena, reservation key, identifier, or lookup operation.
pub trait EnvironmentPreparedMaterialCapability: Send + 'static {
    fn consume(
        self: Box<Self>,
        kind: EnvironmentPreparedMaterialKind,
        alias: MaterialAlias,
        visitor: &mut dyn EnvironmentPreparedMaterialVisitor,
    ) -> AppResult<()>;
}

pub trait EnvironmentPreparedMaterialVisitor {
    fn visit(
        &mut self,
        kind: EnvironmentPreparedMaterialKind,
        alias: MaterialAlias,
        fingerprint: [u8; 32],
        protected_payload: Zeroizing<Vec<u8>>,
    ) -> AppResult<()>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentPreparedMaterialKind {
    Certificate,
    Secret,
}

/// Application-private wrapper. Only Infrastructure can produce its opaque implementation and
/// only Application can place that implementation into a prepared commit aggregate.
pub(in crate::environment_configuration) struct PreparedMaterialCapabilityHandle(
    Box<dyn EnvironmentPreparedMaterialCapability>,
);

impl PreparedMaterialCapabilityHandle {
    pub(in crate::environment_configuration) fn from_capability(
        capability: Box<dyn EnvironmentPreparedMaterialCapability>,
    ) -> Self {
        Self(capability)
    }

    fn consume_with(
        self,
        kind: EnvironmentPreparedMaterialKind,
        alias: MaterialAlias,
        visitor: &mut dyn EnvironmentPreparedMaterialVisitor,
    ) -> AppResult<()> {
        self.0.consume(kind, alias, visitor)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaterialAlias(String);

impl MaterialAlias {
    pub fn parse(value: impl Into<String>) -> AppResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(crate::AppError::new(
                "MATERIAL_ALIAS_INVALID",
                "受保护材料别名无效。",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[expect(
    missing_debug_implementations,
    reason = "contains opaque protected handles"
)]
pub struct EnvironmentPreparedMaterials {
    target: EnvironmentCommitTarget,
    workspace: ProxyWorkspace,
    prepared_certificate_handles: BTreeMap<MaterialAlias, PreparedMaterialCapabilityHandle>,
    prepared_secret_handles: BTreeMap<MaterialAlias, PreparedMaterialCapabilityHandle>,
}

impl EnvironmentPreparedMaterials {
    pub(in crate::environment_configuration) fn new(
        target: EnvironmentCommitTarget,
        workspace: ProxyWorkspace,
        prepared_certificate_handles: BTreeMap<MaterialAlias, PreparedMaterialCapabilityHandle>,
        prepared_secret_handles: BTreeMap<MaterialAlias, PreparedMaterialCapabilityHandle>,
    ) -> Self {
        Self {
            target,
            workspace,
            prepared_certificate_handles,
            prepared_secret_handles,
        }
    }

    #[cfg(test)]
    pub(crate) fn without_materials_for_test(
        target: EnvironmentCommitTarget,
        workspace: ProxyWorkspace,
    ) -> Self {
        Self::new(target, workspace, BTreeMap::new(), BTreeMap::new())
    }

    pub(crate) fn into_commit_request(
        self,
        baseline: EnvironmentApplyGenerations,
        selection_policy: EnvironmentSelectionPolicy,
    ) -> EnvironmentCommitRequest {
        EnvironmentCommitRequest {
            baseline,
            prepared_materials: self,
            selection_policy,
        }
    }

    pub fn consume_with(
        self,
        visitor: &mut dyn EnvironmentPreparedMaterialVisitor,
    ) -> AppResult<EnvironmentConsumedPreparedMaterials> {
        let Self {
            target,
            workspace,
            prepared_certificate_handles,
            prepared_secret_handles,
        } = self;
        for (alias, handle) in prepared_certificate_handles {
            handle.consume_with(EnvironmentPreparedMaterialKind::Certificate, alias, visitor)?;
        }
        for (alias, handle) in prepared_secret_handles {
            handle.consume_with(EnvironmentPreparedMaterialKind::Secret, alias, visitor)?;
        }
        Ok(EnvironmentConsumedPreparedMaterials { target, workspace })
    }
}

#[derive(Debug)]
pub struct EnvironmentConsumedPreparedMaterials {
    pub target: EnvironmentCommitTarget,
    pub workspace: ProxyWorkspace,
}

#[expect(
    missing_debug_implementations,
    reason = "contains opaque protected handles"
)]
pub struct EnvironmentCommitRequest {
    baseline: EnvironmentApplyGenerations,
    prepared_materials: EnvironmentPreparedMaterials,
    selection_policy: EnvironmentSelectionPolicy,
}

#[derive(Debug)]
pub struct EnvironmentConsumedCommitRequest {
    pub baseline: EnvironmentApplyGenerations,
    pub target: EnvironmentCommitTarget,
    pub workspace: ProxyWorkspace,
    pub selection_policy: EnvironmentSelectionPolicy,
}

impl EnvironmentCommitRequest {
    pub fn without_prepared_materials(
        baseline: EnvironmentApplyGenerations,
        target: EnvironmentCommitTarget,
        workspace: ProxyWorkspace,
        selection_policy: EnvironmentSelectionPolicy,
    ) -> Self {
        Self {
            baseline,
            prepared_materials: EnvironmentPreparedMaterials::new(
                target,
                workspace,
                BTreeMap::new(),
                BTreeMap::new(),
            ),
            selection_policy,
        }
    }

    pub fn baseline(&self) -> &EnvironmentApplyGenerations {
        &self.baseline
    }

    pub fn consume_with(
        self,
        visitor: &mut dyn EnvironmentPreparedMaterialVisitor,
    ) -> AppResult<EnvironmentConsumedCommitRequest> {
        let Self {
            baseline,
            prepared_materials,
            selection_policy,
        } = self;
        let EnvironmentConsumedPreparedMaterials { target, workspace } =
            prepared_materials.consume_with(visitor)?;
        Ok(EnvironmentConsumedCommitRequest {
            baseline,
            target,
            workspace,
            selection_policy,
        })
    }
}
