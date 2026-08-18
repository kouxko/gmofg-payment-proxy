use std::{fmt, sync::Arc};

use intercept_proxy_domain::{DocumentSchema, ProtocolPackageRef};

use crate::{
    ProtocolDirection, ProtocolManifest, ProtocolPackageKind,
    compiler::{CompiledDirection, CompiledEntry},
};

/// 已通过完整导入校验并可供数据面冻结引用的协议包编译产物。
#[derive(Clone)]
pub struct CompiledProtocolPackage {
    manifest: ProtocolManifest,
    upstream: CompiledDirection,
    downstream: CompiledDirection,
}

impl CompiledProtocolPackage {
    pub(crate) fn from_compilation(
        manifest: ProtocolManifest,
        upstream: CompiledDirection,
        downstream: CompiledDirection,
    ) -> Self {
        Self {
            manifest,
            upstream,
            downstream,
        }
    }

    /// 返回编译产物绑定的精确协议包 ID 与版本。
    #[must_use]
    pub fn package(&self) -> &ProtocolPackageRef {
        self.manifest.package().package()
    }

    /// 返回协议包所属的数据平面。
    #[must_use]
    pub const fn kind(&self) -> ProtocolPackageKind {
        self.manifest.kind()
    }

    /// 返回指定方向的不可变 Document Schema。
    #[must_use]
    pub fn schema(&self, direction: ProtocolDirection) -> &DocumentSchema {
        self.direction(direction).schema()
    }

    pub(crate) fn schema_arc(&self, direction: ProtocolDirection) -> Arc<DocumentSchema> {
        self.direction(direction).schema_arc()
    }

    /// 返回已经通过编译校验的 Manifest 声明。
    #[must_use]
    pub const fn manifest(&self) -> &ProtocolManifest {
        &self.manifest
    }

    /// 两个方向都必须声明并编译 Encode。
    #[must_use]
    pub const fn supports_upstream_encode(&self) -> bool {
        true
    }

    /// 两个方向都必须声明并编译 Encode。
    #[must_use]
    pub const fn supports_downstream_encode(&self) -> bool {
        true
    }

    /// 两个方向都必须声明并编译 Display。
    #[must_use]
    pub const fn supports_display(&self) -> bool {
        true
    }

    pub(crate) const fn direction(&self, direction: ProtocolDirection) -> &CompiledDirection {
        match direction {
            ProtocolDirection::Upstream => &self.upstream,
            ProtocolDirection::Downstream => &self.downstream,
        }
    }

    pub(crate) const fn upstream(&self) -> &CompiledDirection {
        &self.upstream
    }

    pub(crate) const fn downstream(&self) -> &CompiledDirection {
        &self.downstream
    }

    pub(crate) const fn display(&self, direction: ProtocolDirection) -> &CompiledEntry {
        self.direction(direction).display()
    }
}

impl fmt::Debug for CompiledProtocolPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompiledProtocolPackage")
            .field("package", self.package())
            .field("kind", &self.kind())
            .field("upstream_schema", &self.upstream.schema())
            .field("downstream_schema", &self.downstream.schema())
            .finish_non_exhaustive()
    }
}
