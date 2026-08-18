use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use intercept_proxy_domain::{Document, ProtocolPackageRef};
use rhai::{Blob, Engine, EvalAltResult, ImmutableString, Scope};

use crate::{
    CompiledProtocolPackage, ProtocolDirection, ProtocolEntryPoint, ProtocolExecutionCancellation,
    ProtocolResourceLimit, ProtocolRuntimeError, ProtocolRuntimeLimits, ProtocolRuntimeResult,
    compiler::{CompiledDirection, CompiledEntry, build_engine},
    host::{
        ProtocolHostApi,
        context::{ProtocolCallContext, ProtocolStage},
    },
};

mod helpers;

use helpers::{compiled_direction, exceeds_limit, find_resource_limit, validate_document_schema};

use super::{
    DirectionExecutionPlan, DisplayFallbackReason, ProtocolDisplayResult, ProtocolFrameOutput,
    deadline::CallDeadline,
};

/// 一个连接方向独占的 Decode/Encode/Display Rhai 执行器。
///
/// 每个实例只绑定一个编译包、方向、连接和 Listener。Engine 不会切换 Schema；每个入口调用都使用
/// 全新的 Rhai Scope，并重新武装单次墙钟截止时间。公开方法要求 `&mut self`，让同一方向的调用在
/// 类型层面保持单所有者顺序执行，不会通过共享锁把脚本状态带到另一 Frame。
pub struct ProtocolDirectionExecutor {
    engine: Engine,
    deadline: CallDeadline,
    package: ProtocolPackageRef,
    plan: DirectionExecutionPlan,
    host: ProtocolHostApi,
    decode: CompiledEntry,
    encode: CompiledEntry,
    display: CompiledEntry,
    connection_id: String,
    listener_id: String,
    limits: ProtocolRuntimeLimits,
    output_owner: Arc<()>,
    cancellation: ProtocolExecutionCancellation,
}

impl fmt::Debug for ProtocolDirectionExecutor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Engine/AST 的 Debug 可能暴露脚本结构；运行诊断只保留安全的包、计划和连接标识。
        formatter
            .debug_struct("ProtocolDirectionExecutor")
            .field("package", &self.package)
            .field("plan", &self.plan)
            .field("connection_id", &self.connection_id)
            .field("listener_id", &self.listener_id)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl ProtocolDirectionExecutor {
    /// 构造已绑定单包、单方向、单连接的执行器。
    pub fn new(
        package: &CompiledProtocolPackage,
        plan: DirectionExecutionPlan,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
        limits: ProtocolRuntimeLimits,
    ) -> ProtocolRuntimeResult<Self> {
        Self::new_with_cancellation(
            package,
            plan,
            connection_id,
            listener_id,
            limits,
            ProtocolExecutionCancellation::new(),
        )
    }

    /// 使用调用方提供的共享取消句柄构造执行器。
    ///
    /// 同一连接方向可将该句柄同时注入 Frame Inspector，从而统一取消 Frame、Decode、Encode
    /// 和 Display；句柄 reset 后只允许之后开始的新调用，不能复活已经运行的旧调用。
    pub fn new_with_cancellation(
        package: &CompiledProtocolPackage,
        plan: DirectionExecutionPlan,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
        limits: ProtocolRuntimeLimits,
        cancellation: ProtocolExecutionCancellation,
    ) -> ProtocolRuntimeResult<Self> {
        // DirectionExecutionPlan 是轻量不可变值，调用方可能误把由另一个包派生的计划传进来。必须用
        // 实际执行包重新验证可选入口能力，不能依赖之后的 `expect` 或静默关闭 Encode/Display。
        let plan = DirectionExecutionPlan::new(plan.direction());
        let host = ProtocolHostApi::for_package(package, plan.direction());
        let mut engine = build_engine(limits);
        host.register(&mut engine);
        let deadline = CallDeadline::install(&mut engine, cancellation.clone());
        let direction = compiled_direction(package, plan.direction());
        Ok(Self {
            engine,
            deadline,
            package: package.package().clone(),
            plan,
            host,
            decode: direction.decode().clone(),
            encode: direction.encode().clone(),
            display: package.display(plan.direction()).clone(),
            connection_id: connection_id.into(),
            listener_id: listener_id.into(),
            limits,
            output_owner: Arc::new(()),
            cancellation,
        })
    }

    /// 返回该方向 Frame/Decode/Encode/Display 可共享的取消句柄克隆。
    #[must_use]
    pub fn cancellation(&self) -> ProtocolExecutionCancellation {
        self.cancellation.clone()
    }

    /// 执行完整 Frame，不对 Document 应用额外规则。
    pub fn execute_frame(&mut self, origin: Vec<u8>) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        self.execute_frame_with_rules(origin, |_| Ok(()))
    }

    /// 执行完整 Frame，并在 Decode 后、Encode 前应用 Document 规则。
    ///
    /// 闭包允许规则引擎原位修改 Document，而无需把 Rhai 或 AST 暴露给外层。
    pub fn execute_frame_with_rules(
        &mut self,
        origin: Vec<u8>,
        rules: impl FnOnce(&mut Document) -> ProtocolRuntimeResult<()>,
    ) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        self.execute_frame_with_document_transform(origin, |mut document| {
            rules(&mut document)?;
            Ok(document)
        })
    }

    /// 按 owned Document 边界在 Decode 后、Encode 前执行一次宿主变换。
    ///
    /// owned 输入使宿主可以原子地执行整组规则：失败时不会把半修改对象写回执行器。
    pub fn execute_frame_with_document_transform(
        &mut self,
        origin: Vec<u8>,
        transform: impl FnOnce(Document) -> ProtocolRuntimeResult<Document>,
    ) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        self.ensure_blob_input(ProtocolEntryPoint::Decode, origin.len())?;
        let decoded = self.call_decode(&origin)?;
        validate_document_schema(&decoded, self.host.create_document().schema()).map_err(|()| {
            ProtocolRuntimeError::EntryPointFailed {
                package: self.package.clone(),
                entry: ProtocolEntryPoint::Decode,
            }
        })?;
        self.finish_frame(origin, transform(decoded)?, true)
    }

    /// 仅执行接收侧 Decode 与宿主 Document 变换，不调用同方向 Encode。
    ///
    /// 本地应答的请求不会继续发往远端；其 upstream Encode 不属于该处理流程。返回值仍保留
    /// 原始字节和变换后的 Document，供抓包与 Display 使用。
    pub(super) fn decode_frame_with_document_transform(
        &mut self,
        origin: Vec<u8>,
        transform: impl FnOnce(Document) -> ProtocolRuntimeResult<Document>,
    ) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        self.ensure_blob_input(ProtocolEntryPoint::Decode, origin.len())?;
        let decoded = self.call_decode(&origin)?;
        validate_document_schema(&decoded, self.host.create_document().schema()).map_err(|()| {
            ProtocolRuntimeError::EntryPointFailed {
                package: self.package.clone(),
                entry: ProtocolEntryPoint::Decode,
            }
        })?;
        let transformed = transform(decoded)?;
        validate_document_schema(&transformed, self.host.create_document().schema()).map_err(
            |()| ProtocolRuntimeError::EntryPointFailed {
                package: self.package.clone(),
                entry: ProtocolEntryPoint::Decode,
            },
        )?;
        Ok(ProtocolFrameOutput::new(
            Arc::clone(&self.output_owner),
            origin.clone(),
            origin,
            Some(transformed.clone()),
            transformed,
        ))
    }

    /// 解码文本承载的协议报文并执行 Document 变换；仅在 Document 实际改变时调用 Encode。
    ///
    /// HTTP Body 使用该入口保证“没有规则改变 Document”时逐字节保留原文。调用方负责在进入
    /// 本方法前确认输入为 UTF-8，并在采用 `written` 前确认 Encode 输出仍是 UTF-8。
    pub fn execute_message_with_document_transform(
        &mut self,
        origin: Vec<u8>,
        transform: impl FnOnce(Document) -> ProtocolRuntimeResult<Document>,
    ) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        self.ensure_blob_input(ProtocolEntryPoint::Decode, origin.len())?;
        let decoded = self.call_decode(&origin)?;
        validate_document_schema(&decoded, self.host.create_document().schema()).map_err(|()| {
            ProtocolRuntimeError::EntryPointFailed {
                package: self.package.clone(),
                entry: ProtocolEntryPoint::Decode,
            }
        })?;
        let mut transformed = transform(decoded.clone())?;
        validate_document_schema(&transformed, self.host.create_document().schema()).map_err(
            |()| ProtocolRuntimeError::EntryPointFailed {
                package: self.package.clone(),
                entry: ProtocolEntryPoint::Decode,
            },
        )?;
        let written = if transformed == decoded {
            origin.clone()
        } else {
            self.ensure_blob_input(ProtocolEntryPoint::Encode, origin.len())?;
            self.call_encode(&origin, &mut transformed)?
        };
        Ok(ProtocolFrameOutput::new(
            Arc::clone(&self.output_owner),
            origin,
            written,
            Some(decoded),
            transformed,
        ))
    }

    /// 跳过 Decode，以调用方提供的同 Schema owned Document 直接完成 Encode/Echo。
    ///
    /// `LocalResponder` 的 response 没有真实 downstream 输入流，所以不能调用 downstream Frame/Decode。
    /// 此入口只对 crate 内协调器开放，并再次验证 Document 身份，防止规则闭包替换 Schema。
    pub(super) fn execute_predecoded_document(
        &mut self,
        origin: Vec<u8>,
        document: Document,
    ) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        self.finish_frame(origin, document, false)
    }

    fn finish_frame(
        &mut self,
        origin: Vec<u8>,
        mut document: Document,
        decoded: bool,
    ) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        // 规则实现也只能通过 Domain API 修改 Document；这里再次验证身份，防止未来闭包边界扩展后
        // 把其他包的 Document 带入 Encode。
        validate_document_schema(&document, self.host.create_document().schema()).map_err(
            |()| ProtocolRuntimeError::EntryPointFailed {
                package: self.package.clone(),
                entry: ProtocolEntryPoint::Decode,
            },
        )?;
        let decoded_document = decoded.then(|| document.clone());
        self.ensure_blob_input(ProtocolEntryPoint::Encode, origin.len())?;
        let written = self.call_encode(&origin, &mut document)?;
        Ok(ProtocolFrameOutput::new(
            Arc::clone(&self.output_owner),
            origin,
            written,
            decoded_document,
            document,
        ))
    }

    /// 在网络输出已经确定后尝试生成 UI HTML；任何 Display 失败都只返回 Hex 回退。
    #[must_use]
    pub fn render_display(&mut self, output: &ProtocolFrameOutput) -> ProtocolDisplayResult {
        // Output 只能交回生成它的同一执行器。即使两个包恰好声明相同 Schema，也不能把另一包或
        // 另一连接的 Document 放进当前 Display Context，避免跨连接数据混淆与泄露。
        if !output.belongs_to(&self.output_owner) {
            return ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed);
        }
        self.render_output_document_display(output)
    }

    /// 对已经成功 Decode 的输入 Document 调用公共 Display。
    #[must_use]
    pub fn render_decoded_display(
        &mut self,
        output: &ProtocolFrameOutput,
    ) -> ProtocolDisplayResult {
        if !output.belongs_to(&self.output_owner) || output.decoded_document().is_none() {
            return ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed);
        }
        self.render_output_document_display(output)
    }

    /// 对该执行器产生的 Document 调用公共 Display。
    #[must_use]
    pub fn render_output_document_display(
        &mut self,
        output: &ProtocolFrameOutput,
    ) -> ProtocolDisplayResult {
        if !output.belongs_to(&self.output_owner) {
            return ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed);
        }
        match self.call_display(output.execution_document()) {
            Ok(html) => ProtocolDisplayResult::UntrustedHtml(html),
            Err(ProtocolRuntimeError::ResourceLimitExceeded { limit, .. }) => {
                ProtocolDisplayResult::HexFallback(DisplayFallbackReason::ResourceLimitExceeded(
                    limit,
                ))
            }
            Err(_) => ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed),
        }
    }

    /// 对当前方向、当前 Schema 的中间 Document 生成 UI HTML。
    ///
    /// HTTP 会在同一网络方向内顺序执行两段规则；该入口让调用方分别冻结每段规则执行后的
    /// Document 与 Display，而不会把中间状态误当作最终网络输出。
    #[must_use]
    pub fn render_document_display(&mut self, document: &Document) -> ProtocolDisplayResult {
        if validate_document_schema(document, self.host.create_document().schema()).is_err() {
            return ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed);
        }
        match self.call_display(document) {
            Ok(html) => ProtocolDisplayResult::UntrustedHtml(html),
            Err(ProtocolRuntimeError::ResourceLimitExceeded { limit, .. }) => {
                ProtocolDisplayResult::HexFallback(DisplayFallbackReason::ResourceLimitExceeded(
                    limit,
                ))
            }
            Err(_) => ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed),
        }
    }

    fn call_decode(&mut self, origin: &[u8]) -> ProtocolRuntimeResult<Document> {
        let context = self.context(ProtocolStage::Receive);
        let entry = self.decode.clone();
        let started = self.arm_deadline(ProtocolEntryPoint::Decode)?;
        let result = self.engine.call_fn::<Document>(
            &mut Scope::new(),
            entry.ast(),
            entry.function().as_str(),
            (origin.to_vec(), context),
        );
        self.finish_call(ProtocolEntryPoint::Decode, started, result)
    }

    fn call_encode(
        &mut self,
        origin: &[u8],
        document: &mut Document,
    ) -> ProtocolRuntimeResult<Vec<u8>> {
        let entry = self.encode.clone();
        let context = self.context(ProtocolStage::Send);
        let started = self.arm_deadline(ProtocolEntryPoint::Encode)?;
        let result = self.engine.call_fn::<Blob>(
            &mut Scope::new(),
            entry.ast(),
            entry.function().as_str(),
            (origin.to_vec(), document.clone(), context),
        );
        let blob = self.finish_call(ProtocolEntryPoint::Encode, started, result)?;
        self.ensure_blob_output(ProtocolEntryPoint::Encode, blob.len())?;
        Ok(blob)
    }

    fn call_display(&mut self, document: &Document) -> ProtocolRuntimeResult<String> {
        let entry = self.display.clone();
        let context = self.context(ProtocolStage::Display);
        let started = self.arm_deadline(ProtocolEntryPoint::Display)?;
        let result = self.engine.call_fn::<ImmutableString>(
            &mut Scope::new(),
            entry.ast(),
            entry.function().as_str(),
            (document.clone(), context),
        );
        let html = self.finish_call(ProtocolEntryPoint::Display, started, result)?;
        if exceeds_limit(html.len(), self.limits.max_string_bytes()) {
            return Err(self.resource_error(
                ProtocolEntryPoint::Display,
                ProtocolResourceLimit::StringBytes,
            ));
        }
        Ok(html.into_owned())
    }

    fn context(&self, stage: ProtocolStage) -> ProtocolCallContext {
        ProtocolCallContext::new(
            self.plan.direction(),
            stage,
            self.connection_id.clone(),
            self.listener_id.clone(),
        )
    }

    fn arm_deadline(&self, entry: ProtocolEntryPoint) -> ProtocolRuntimeResult<Instant> {
        self.deadline
            .arm(Duration::from_millis(self.limits.max_wall_time_ms()))
            .map_err(|()| self.cancellation_error(entry))
    }

    fn finish_call<T>(
        &self,
        entry: ProtocolEntryPoint,
        started: Instant,
        result: Result<T, Box<EvalAltResult>>,
    ) -> ProtocolRuntimeResult<T> {
        let cancelled = self.deadline.was_cancelled();
        self.deadline.disarm();
        if cancelled {
            return Err(self.cancellation_error(entry));
        }
        match result {
            Err(error) => Err(self.map_eval_error(entry, &error)),
            Ok(_) if started.elapsed() > Duration::from_millis(self.limits.max_wall_time_ms()) => {
                Err(self.resource_error(entry, ProtocolResourceLimit::WallTimeMs))
            }
            Ok(value) => Ok(value),
        }
    }

    fn map_eval_error(
        &self,
        entry: ProtocolEntryPoint,
        error: &EvalAltResult,
    ) -> ProtocolRuntimeError {
        find_resource_limit(error).map_or_else(
            || ProtocolRuntimeError::EntryPointFailed {
                package: self.package.clone(),
                entry,
            },
            |limit| self.resource_error(entry, limit),
        )
    }

    fn ensure_blob_input(
        &self,
        entry: ProtocolEntryPoint,
        length: usize,
    ) -> ProtocolRuntimeResult<()> {
        self.ensure_blob_output(entry, length)
    }

    fn ensure_blob_output(
        &self,
        entry: ProtocolEntryPoint,
        length: usize,
    ) -> ProtocolRuntimeResult<()> {
        if exceeds_limit(length, self.limits.max_blob_bytes()) {
            Err(self.resource_error(entry, ProtocolResourceLimit::BlobBytes))
        } else {
            Ok(())
        }
    }

    fn resource_error(
        &self,
        entry: ProtocolEntryPoint,
        limit: ProtocolResourceLimit,
    ) -> ProtocolRuntimeError {
        ProtocolRuntimeError::ResourceLimitExceeded {
            package: self.package.clone(),
            entry,
            limit,
        }
    }

    fn cancellation_error(&self, entry: ProtocolEntryPoint) -> ProtocolRuntimeError {
        ProtocolRuntimeError::ExecutionCancelled {
            package: self.package.clone(),
            entry,
        }
    }
}

#[cfg(test)]
mod tests;
