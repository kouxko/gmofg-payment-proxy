use std::sync::Arc;

use intercept_proxy_domain::{DocumentSchema, ProtocolPackageRef};

/// 已通过完整导入校验并可供 Listener 冻结引用的协议包编译产物。
///
/// 字段全部私有且本 crate 暂不暴露公开构造器，因此外层无法只拿 Manifest 元数据伪造“已编译”状态。
/// T05 先保存后续各阶段共同需要的精确包身份与不可变 Schema；真正的 Rhai AST 和方向入口将在 T08
/// 由编译器写入私有字段，而不改变外部句柄语义。
#[derive(Clone, Debug)]
pub struct CompiledProtocolPackage {
    package: ProtocolPackageRef,
    schema: Arc<DocumentSchema>,
}

impl CompiledProtocolPackage {
    // T05 没有真实编译器，生产构建不能构造该句柄；测试构造器只验证稳定元数据边界。
    // T08 会把此入口替换为仅编译器可调用、同时要求 AST/方向入口的内部构造器。
    #[cfg(test)]
    pub(crate) fn new(package: ProtocolPackageRef, schema: impl Into<Arc<DocumentSchema>>) -> Self {
        Self {
            package,
            schema: schema.into(),
        }
    }

    /// 返回编译产物绑定的精确协议包 ID 与版本。
    #[must_use]
    pub const fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    /// 返回编译产物共享的不可变 Document Schema。
    #[must_use]
    pub fn schema(&self) -> &DocumentSchema {
        &self.schema
    }
}
