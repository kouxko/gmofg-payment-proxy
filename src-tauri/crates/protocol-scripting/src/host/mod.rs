//! Rhai Host API v1 的类型注册边界。
//!
//! 本模块只把领域层已经验证的 [`Document`](intercept_proxy_domain::Document) 与当前调用的只读
//! [`ProtocolCallContext`] 注册给 Rhai。只读 Reader 与 `FramingDecision` 构造器也在同一固定注册表；
//! framing 和 runtime 执行器各自只调用自己负责的入口。

pub(crate) mod context;
mod document;

use std::sync::Arc;

use intercept_proxy_domain::DocumentSchemaNode;
use rhai::Engine;

use crate::framing;
use crate::{CompiledProtocolPackage, ProtocolDirection};

/// 与一个不可变 Document Schema 绑定的 Host API 注册器。
///
/// `document::create()` 不接受脚本提供的 Schema 参数，而是始终克隆这里保存的 `Arc`，因此脚本无法
/// 创建其他协议包的 Document。注册器本身不保存 Document 或 Context，也就没有跨 Frame 状态。
#[derive(Clone, Debug)]
pub(crate) struct ProtocolHostApi {
    schema: Arc<DocumentSchemaNode>,
}

impl ProtocolHostApi {
    /// 从已通过完整编译校验的协议包建立 Host API。
    pub(crate) fn for_package(
        package: &CompiledProtocolPackage,
        direction: ProtocolDirection,
    ) -> Self {
        Self {
            schema: package.schema_arc(direction),
        }
    }

    /// 把 Document 与 Context 的固定函数集合安装到受限 Engine。
    ///
    /// 调用方应为每个协议包使用独立 Engine；重复注册另一个 Schema 会替换 `document` 静态模块，
    /// 因而执行器不会在共享 Engine 上动态切换协议包。
    pub(crate) fn register(&self, engine: &mut Engine) {
        document::register(engine, Arc::clone(&self.schema));
        context::register(engine);
        framing::register(engine);
    }
}
