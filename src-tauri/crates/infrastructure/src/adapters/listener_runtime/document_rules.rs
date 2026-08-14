//! 每连接绑定的 Socket Document 规则运行时外壳。
//!
//! Domain Program 负责纯规则语义；本模块只补齐 Domain 不应认识的网络连接身份。这样未来
//! T24/T25 可以把 Decode 产生的 owned Document 交给规则程序，同时在真正执行前拒绝跨连接、
//! 跨 Listener、跨包或跨方向串用。这里不进行网络 I/O，也不保存上一 Frame 的 Document。

use std::{fmt, sync::Arc};

use intercept_proxy_domain::{
    Document, DomainError, ErrorCode, ListenerId, ProtocolPackageRef, SocketDirection,
    SocketDocumentRuleExecution, SocketDocumentRuleProgram,
};
use intercept_proxy_runtime::SocketConnectionIdentity;

/// 一次 Listener 启动后冻结的双方向 Document 规则连接工厂。
///
/// 工厂持有快照编译出的不可变 Program；每个已接纳连接只需提供自己的 identity，便可获得
/// 独立的方向执行入口。这样生产路径不会重新编译规则，也不会共享任何 Frame Document 状态。
#[derive(Clone)]
pub struct SocketDocumentRuleConnectionFactory {
    upstream: Arc<SocketDocumentRuleProgram>,
    downstream: Arc<SocketDocumentRuleProgram>,
}

/// 一个连接、一个方向独占的规则执行入口。
///
/// Program 自身不可变，可在不同连接间共享；Connection 只保存无 payload 的 owner identity，
/// 每次调用的 Document 和命中列表都属于返回值，因此没有跨 Frame 的可变状态。
pub struct SocketDocumentRuleConnection {
    connection: SocketConnectionIdentity,
    program: Arc<SocketDocumentRuleProgram>,
}

/// Decode 后等待规则处理的 owned Document 及其完整运行时归属。
///
/// 字段保持私有，生产调用方只能通过绑定它的 Connection 创建；执行时仍会逐项复核，防止未来
/// 适配器扩展或错误重构把另一个连接/包/方向的 Document 交给当前 Program。
pub struct BoundSocketDocument {
    connection: SocketConnectionIdentity,
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    direction: SocketDirection,
    document: Document,
}

impl fmt::Debug for SocketDocumentRuleConnectionFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketDocumentRuleConnectionFactory")
            .field("listener_id", &self.upstream.listener_id())
            .field("package", self.upstream.package())
            .field("schema_id", &self.upstream.schema().id())
            .field("schema_version", &self.upstream.schema().version())
            .field("upstream_rule_count", &self.upstream.rules().len())
            .field("downstream_rule_count", &self.downstream.rules().len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for SocketDocumentRuleConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketDocumentRuleConnection")
            .field("connection", &self.connection)
            .field("program", &self.program)
            .finish()
    }
}

impl fmt::Debug for BoundSocketDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Document 的 Debug 含字段值；运行诊断只能暴露归属与 Schema 形状。
        formatter
            .debug_struct("BoundSocketDocument")
            .field("connection", &self.connection)
            .field("listener_id", &self.listener_id)
            .field("package", &self.package)
            .field("direction", &self.direction)
            .field("schema_id", &self.document.schema().id())
            .field("schema_version", &self.document.schema().version())
            .finish_non_exhaustive()
    }
}

impl SocketDocumentRuleConnection {
    /// 为一个不可变规则 Program 绑定精确 Socket 连接身份。
    pub(crate) fn new(
        connection: SocketConnectionIdentity,
        program: Arc<SocketDocumentRuleProgram>,
    ) -> Self {
        Self {
            connection,
            program,
        }
    }

    /// 绑定 Decode 产生的 owned Document；完整 Schema 在执行边界由 Program 复核。
    pub fn bind_document(&self, document: Document) -> BoundSocketDocument {
        BoundSocketDocument {
            connection: self.connection.clone(),
            listener_id: self.program.listener_id(),
            package: self.program.package().clone(),
            direction: self.program.direction(),
            document,
        }
    }

    /// 为 Decode 关闭的 `LocalResponder` 创建 Schema-bound 空 Document。
    ///
    /// 每次调用都创建新的值槽，Always + `SetField` 可以生成静态响应，而带字段条件的规则会因
    /// 未赋值稳定 non-match；上一 request 的值不会被复用。
    pub fn empty_document(&self) -> BoundSocketDocument {
        self.bind_document(Document::new(self.program.schema().clone()))
    }

    /// 复核运行时归属后执行整组规则，并只返回一个聚合结果。
    pub fn execute(
        &self,
        document: BoundSocketDocument,
    ) -> Result<SocketDocumentRuleExecution, DomainError> {
        if document.connection != self.connection {
            return Err(binding_error(
                "binding.connection",
                "Document 不属于当前 Socket 连接",
            ));
        }
        if document.listener_id != self.program.listener_id() {
            return Err(binding_error(
                "binding.listener_id",
                "Document 不属于当前 Listener",
            ));
        }
        if &document.package != self.program.package() {
            return Err(binding_error(
                "binding.package",
                "Document 不属于当前协议包版本",
            ));
        }
        if document.direction != self.program.direction() {
            return Err(binding_error(
                "binding.direction",
                "Document 不属于当前处理方向",
            ));
        }
        self.program.execute(document.document)
    }
}

impl SocketDocumentRuleConnectionFactory {
    /// 组合同一 Listener、协议包和完整 Schema 的 upstream/downstream Program。
    pub(crate) fn new(
        upstream: Arc<SocketDocumentRuleProgram>,
        downstream: Arc<SocketDocumentRuleProgram>,
    ) -> Result<Self, DomainError> {
        if upstream.direction() != SocketDirection::Upstream {
            return Err(binding_error(
                "factory.upstream.direction",
                "upstream Program 方向不正确",
            ));
        }
        if downstream.direction() != SocketDirection::Downstream {
            return Err(binding_error(
                "factory.downstream.direction",
                "downstream Program 方向不正确",
            ));
        }
        if upstream.listener_id() != downstream.listener_id() {
            return Err(binding_error(
                "factory.listener_id",
                "两个方向的 Program 不属于同一 Listener",
            ));
        }
        if upstream.package() != downstream.package() {
            return Err(binding_error(
                "factory.package",
                "两个方向的 Program 不属于同一协议包版本",
            ));
        }
        if upstream.schema() != downstream.schema() {
            return Err(binding_error(
                "factory.schema",
                "两个方向的 Program 没有绑定同一完整 Schema",
            ));
        }
        Ok(Self {
            upstream,
            downstream,
        })
    }

    /// 为一个连接和一个处理方向创建无状态执行入口。
    pub fn connection(
        &self,
        connection: SocketConnectionIdentity,
        direction: SocketDirection,
    ) -> SocketDocumentRuleConnection {
        SocketDocumentRuleConnection::new(connection, Arc::clone(self.program(direction)))
    }

    /// 返回启动快照中冻结的指定方向 Program。
    pub(crate) fn program(&self, direction: SocketDirection) -> &Arc<SocketDocumentRuleProgram> {
        match direction {
            SocketDirection::Upstream => &self.upstream,
            SocketDirection::Downstream => &self.downstream,
        }
    }
}

fn binding_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "Socket Document 运行时绑定不一致")
        .with_field_error(field, message)
}

#[cfg(test)]
mod tests;
