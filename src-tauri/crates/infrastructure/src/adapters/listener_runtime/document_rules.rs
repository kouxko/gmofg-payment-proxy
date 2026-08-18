//! 每连接绑定的 Socket Document 规则运行时外壳。
//!
//! Domain Program 负责纯规则语义；本模块只补齐 Domain 不应认识的网络连接身份。这样未来
//! T24/T25 可以把 Decode 产生的 owned Document 交给规则程序，同时在真正执行前拒绝跨连接、
//! 跨 Listener、跨包或跨方向串用。这里不进行网络 I/O，也不保存上一 Frame 的 Document。

use std::{fmt, sync::Arc};

use intercept_proxy_domain::{
    Document, DomainError, ErrorCode, ListenerId, ProtocolPackageRef, SocketDocumentRuleExecution,
    SocketDocumentRuleProgram, SocketRuleStage,
};
use intercept_proxy_runtime::SocketConnectionIdentity;
use parking_lot::RwLock;

/// 一个入口运行期间可原子替换的双方向 Document 规则连接工厂。
///
/// 每组 Program 都不可变；保存规则时一次性替换整组。每个连接、每条报文执行前读取当前组，
/// 因此已有连接也能使用新规则，同时不会共享任何报文 Document 状态。
#[derive(Clone)]
pub struct SocketDocumentRuleConnectionFactory {
    programs: Arc<RwLock<SocketDocumentRulePrograms>>,
}

#[derive(Clone)]
struct SocketDocumentRulePrograms {
    app_to_proxy: Arc<SocketDocumentRuleProgram>,
    proxy_to_upstream: Arc<SocketDocumentRuleProgram>,
    upstream_to_proxy: Arc<SocketDocumentRuleProgram>,
    proxy_to_app: Arc<SocketDocumentRuleProgram>,
}

/// 一个连接、一个方向独占的规则执行入口。
///
/// Program 自身不可变，可在不同连接间共享；Connection 只保存无 payload 的 owner identity，
/// 每次调用的 Document 和命中列表都属于返回值，因此没有跨 Frame 的可变状态。
pub struct SocketDocumentRuleConnection {
    connection: SocketConnectionIdentity,
    programs: Arc<RwLock<SocketDocumentRulePrograms>>,
    stage: SocketRuleStage,
}

/// Decode 后等待规则处理的 owned Document 及其完整运行时归属。
///
/// 字段保持私有，生产调用方只能通过绑定它的 Connection 创建；执行时仍会逐项复核，防止未来
/// 适配器扩展或错误重构把另一个连接/包/方向的 Document 交给当前 Program。
pub struct BoundSocketDocument {
    connection: SocketConnectionIdentity,
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    stage: SocketRuleStage,
    document: Document,
}

impl fmt::Debug for SocketDocumentRuleConnectionFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let programs = self.programs.read();
        formatter
            .debug_struct("SocketDocumentRuleConnectionFactory")
            .field("listener_id", &programs.app_to_proxy.listener_id())
            .field("package", programs.app_to_proxy.package())
            .field("schema_id", &programs.app_to_proxy.schema().id())
            .field("schema_version", &programs.app_to_proxy.schema().version())
            .field("app_to_proxy", &programs.app_to_proxy.rules().len())
            .field(
                "proxy_to_upstream",
                &programs.proxy_to_upstream.rules().len(),
            )
            .field(
                "upstream_to_proxy",
                &programs.upstream_to_proxy.rules().len(),
            )
            .field("proxy_to_app", &programs.proxy_to_app.rules().len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for SocketDocumentRuleConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let program = self.program();
        formatter
            .debug_struct("SocketDocumentRuleConnection")
            .field("connection", &self.connection)
            .field("stage", &self.stage)
            .field("listener_id", &program.listener_id())
            .field("package", program.package())
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
            .field("stage", &self.stage)
            .field("schema_id", &self.document.schema().id())
            .field("schema_version", &self.document.schema().version())
            .finish_non_exhaustive()
    }
}

impl SocketDocumentRuleConnection {
    /// 为一个不可变规则 Program 绑定精确 Socket 连接身份。
    fn new(
        connection: SocketConnectionIdentity,
        programs: Arc<RwLock<SocketDocumentRulePrograms>>,
        stage: SocketRuleStage,
    ) -> Self {
        Self {
            connection,
            programs,
            stage,
        }
    }

    fn program(&self) -> Arc<SocketDocumentRuleProgram> {
        let programs = self.programs.read();
        match self.stage {
            SocketRuleStage::AppToProxy => Arc::clone(&programs.app_to_proxy),
            SocketRuleStage::ProxyToUpstream => Arc::clone(&programs.proxy_to_upstream),
            SocketRuleStage::UpstreamToProxy => Arc::clone(&programs.upstream_to_proxy),
            SocketRuleStage::ProxyToApp => Arc::clone(&programs.proxy_to_app),
        }
    }

    /// 绑定 Decode 产生的 owned Document；完整 Schema 在执行边界由 Program 复核。
    pub fn bind_document(&self, document: Document) -> BoundSocketDocument {
        let program = self.program();
        BoundSocketDocument {
            connection: self.connection.clone(),
            listener_id: program.listener_id(),
            package: program.package().clone(),
            stage: program.stage(),
            document,
        }
    }

    /// 为 Decode 关闭的 `LocalResponder` 创建 Schema-bound 空 Document。
    ///
    /// 每次调用都创建新的值槽，Always + `SetField` 可以生成静态响应，而带字段条件的规则会因
    /// 未赋值稳定 non-match；上一 request 的值不会被复用。
    pub fn empty_document(&self) -> BoundSocketDocument {
        self.bind_document(Document::new(self.program().schema().clone()))
    }

    /// 复核运行时归属后执行整组规则，并只返回一个聚合结果。
    pub fn execute(
        &self,
        document: BoundSocketDocument,
    ) -> Result<SocketDocumentRuleExecution, DomainError> {
        self.execute_with_cancellation(document, || false)
    }

    /// 复核归属后，以调用方提供的同步取消检查执行规则。
    ///
    /// 取消检查由 Domain 在每条规则、条件和动作边界调用；失败时 owned Document 被整体丢弃。
    pub fn execute_with_cancellation(
        &self,
        document: BoundSocketDocument,
        is_cancelled: impl FnMut() -> bool,
    ) -> Result<SocketDocumentRuleExecution, DomainError> {
        let program = self.program();
        if document.connection != self.connection {
            return Err(binding_error(
                "binding.connection",
                "Document 不属于当前 Socket 连接",
            ));
        }
        if document.listener_id != program.listener_id() {
            return Err(binding_error(
                "binding.listener_id",
                "Document 不属于当前 Listener",
            ));
        }
        if &document.package != program.package() {
            return Err(binding_error(
                "binding.package",
                "Document 不属于当前协议包版本",
            ));
        }
        if document.stage != program.stage() {
            return Err(binding_error(
                "binding.stage",
                "Document 不属于当前处理阶段",
            ));
        }
        program.execute_with_cancellation(document.document, is_cancelled)
    }
}

impl SocketDocumentRuleConnectionFactory {
    /// 组合同一入口、协议包和完整 Schema 的四阶段 Program。
    pub(crate) fn new(
        app_to_proxy: Arc<SocketDocumentRuleProgram>,
        proxy_to_upstream: Arc<SocketDocumentRuleProgram>,
        upstream_to_proxy: Arc<SocketDocumentRuleProgram>,
        proxy_to_app: Arc<SocketDocumentRuleProgram>,
    ) -> Result<Self, DomainError> {
        let programs = [
            &app_to_proxy,
            &proxy_to_upstream,
            &upstream_to_proxy,
            &proxy_to_app,
        ];
        let expected = [
            SocketRuleStage::AppToProxy,
            SocketRuleStage::ProxyToUpstream,
            SocketRuleStage::UpstreamToProxy,
            SocketRuleStage::ProxyToApp,
        ];
        for (program, stage) in programs.iter().zip(expected) {
            if program.stage() != stage {
                return Err(binding_error(
                    "factory.stage",
                    "规则 Program 处理阶段不正确",
                ));
            }
            if program.listener_id() != app_to_proxy.listener_id()
                || program.package() != app_to_proxy.package()
                || program.schema() != app_to_proxy.schema()
            {
                return Err(binding_error(
                    "factory.binding",
                    "四个处理阶段必须绑定同一入口、协议包和 Schema",
                ));
            }
        }
        Ok(Self {
            programs: Arc::new(RwLock::new(SocketDocumentRulePrograms {
                app_to_proxy,
                proxy_to_upstream,
                upstream_to_proxy,
                proxy_to_app,
            })),
        })
    }

    /// 为一个连接和一个处理方向创建无状态执行入口。
    pub fn connection(
        &self,
        connection: SocketConnectionIdentity,
        stage: SocketRuleStage,
    ) -> SocketDocumentRuleConnection {
        SocketDocumentRuleConnection::new(connection, Arc::clone(&self.programs), stage)
    }

    /// 返回当前指定方向 Program。
    #[cfg(test)]
    pub(crate) fn program(&self, stage: SocketRuleStage) -> Arc<SocketDocumentRuleProgram> {
        let programs = self.programs.read();
        match stage {
            SocketRuleStage::AppToProxy => Arc::clone(&programs.app_to_proxy),
            SocketRuleStage::ProxyToUpstream => Arc::clone(&programs.proxy_to_upstream),
            SocketRuleStage::UpstreamToProxy => Arc::clone(&programs.upstream_to_proxy),
            SocketRuleStage::ProxyToApp => Arc::clone(&programs.proxy_to_app),
        }
    }

    /// 原子替换双方向规则；已有连接下一条报文会读取新 Program。
    pub(crate) fn replace(&self, replacement: Self) {
        let replacement = replacement.programs.read().clone();
        *self.programs.write() = replacement;
    }
}

fn binding_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "Socket Document 运行时绑定不一致")
        .with_field_error(field, message)
}

#[cfg(test)]
mod tests;
