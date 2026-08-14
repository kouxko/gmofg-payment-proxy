use std::{fmt, sync::Arc};

use intercept_proxy_domain::{DocumentSchema, ProtocolPackageRef};

use crate::{
    ProtocolManifest,
    compiler::{CompiledDirection, CompiledEntry},
};

/// 已通过完整导入校验并可供 Listener 冻结引用的协议包编译产物。
///
/// 字段全部私有且没有公开构造器。唯一生产构造路径是 [`crate::ProtocolPackageCompiler`] 完整通过
/// Manifest、Schema、Rhai 模块和入口校验之后调用内部构造器，因此外层无法伪造“已编译”状态。
#[derive(Clone)]
pub struct CompiledProtocolPackage {
    manifest: ProtocolManifest,
    schema: Arc<DocumentSchema>,
    upstream: CompiledDirection,
    downstream: CompiledDirection,
    display: Option<CompiledEntry>,
}

impl CompiledProtocolPackage {
    pub(crate) fn from_compilation(
        manifest: ProtocolManifest,
        schema: Arc<DocumentSchema>,
        upstream: CompiledDirection,
        downstream: CompiledDirection,
        display: Option<CompiledEntry>,
    ) -> Self {
        Self {
            manifest,
            schema,
            upstream,
            downstream,
            display,
        }
    }

    /// 返回编译产物绑定的精确协议包 ID 与版本。
    #[must_use]
    pub fn package(&self) -> &ProtocolPackageRef {
        self.manifest.package().package()
    }

    /// 返回编译产物共享的不可变 Document Schema。
    #[must_use]
    pub fn schema(&self) -> &DocumentSchema {
        &self.schema
    }

    /// 为同 crate 的 Host/Executor 共享 Schema 所有权，不把可变性暴露给外层。
    pub(crate) fn schema_arc(&self) -> Arc<DocumentSchema> {
        Arc::clone(&self.schema)
    }

    /// 返回已经通过编译校验的 Manifest 声明；不包含脚本源码或 AST。
    #[must_use]
    pub const fn manifest(&self) -> &ProtocolManifest {
        &self.manifest
    }

    /// 返回协议包是否声明并成功编译公共 Display 入口。
    #[must_use]
    pub const fn supports_display(&self) -> bool {
        self.display.is_some()
    }

    /// 返回 Upstream 是否声明并成功编译 Encode 入口。
    #[must_use]
    pub const fn supports_upstream_encode(&self) -> bool {
        self.upstream.encode().is_some()
    }

    /// 返回 Downstream 是否声明并成功编译 Encode 入口。
    #[must_use]
    pub const fn supports_downstream_encode(&self) -> bool {
        self.downstream.encode().is_some()
    }

    // 冻结 AST 只通过 crate 内的 Framing/Runtime 执行器调用，不向外暴露脚本结构。
    pub(crate) const fn upstream(&self) -> &CompiledDirection {
        &self.upstream
    }

    pub(crate) const fn downstream(&self) -> &CompiledDirection {
        &self.downstream
    }

    pub(crate) const fn display(&self) -> Option<&CompiledEntry> {
        self.display.as_ref()
    }
}

impl fmt::Debug for CompiledProtocolPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 不委托 AST 的 Debug，避免诊断日志意外包含脚本结构或字面量。
        formatter
            .debug_struct("CompiledProtocolPackage")
            .field("package", self.package())
            .field("schema", &self.schema)
            .field("supports_display", &self.supports_display())
            .field("supports_upstream_encode", &self.supports_upstream_encode())
            .field(
                "supports_downstream_encode",
                &self.supports_downstream_encode(),
            )
            .finish_non_exhaustive()
    }
}
