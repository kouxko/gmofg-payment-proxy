use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use intercept_proxy_domain::{DirectionProcessingOptions, Document, ProtocolPackageRef};
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
    encode: Option<CompiledEntry>,
    display: Option<CompiledEntry>,
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
        let plan = DirectionExecutionPlan::new(
            package,
            plan.direction(),
            DirectionProcessingOptions {
                decode_enabled: plan.decode_enabled(),
                encode_enabled: plan.encode_enabled(),
            },
        )?;
        let host = ProtocolHostApi::for_package(package);
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
            encode: direction.encode().cloned(),
            display: package.display().cloned(),
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

    /// 按当前四态计划执行完整 Frame，不对 Document 应用额外规则。
    pub fn execute_frame(&mut self, origin: Vec<u8>) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        self.execute_frame_with_rules(origin, |_| Ok(()))
    }

    /// 按当前四态计划执行完整 Frame，并在 Decode 后、Encode 前应用 Document 规则。
    ///
    /// `rules` 只在 Decode 开启且成功时调用；Decode 关闭的 Encode-only 状态使用空 Document，且绝不
    /// 调用规则。闭包允许未来 Socket 规则引擎原位修改 Document，而无需把 Rhai 或 AST 暴露给外层。
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
    /// 变换只在 Decode 开启时调用；Decode 关闭时 Encode 仍收到当前 Schema 的空 Document。
    /// owned 输入使宿主可以原子地执行整组规则：失败时不会把半修改对象写回执行器。
    pub fn execute_frame_with_document_transform(
        &mut self,
        origin: Vec<u8>,
        transform: impl FnOnce(Document) -> ProtocolRuntimeResult<Document>,
    ) -> ProtocolRuntimeResult<ProtocolFrameOutput> {
        let document = if self.plan.decode_enabled() {
            self.ensure_blob_input(ProtocolEntryPoint::Decode, origin.len())?;
            let decoded = self.call_decode(&origin)?;
            validate_document_schema(&decoded, self.host.create_document().schema()).map_err(
                |()| ProtocolRuntimeError::EntryPointFailed {
                    package: self.package.clone(),
                    entry: ProtocolEntryPoint::Decode,
                },
            )?;
            transform(decoded)?
        } else {
            self.host.create_document()
        };

        self.finish_frame(origin, document, self.plan.decode_enabled())
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
        let written = if self.plan.encode_enabled() {
            self.ensure_blob_input(ProtocolEntryPoint::Encode, origin.len())?;
            self.call_encode(&origin, &mut document)?
        } else {
            origin.clone()
        };
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
        if !self.plan.encode_enabled() {
            return ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled);
        }
        if !self.plan.display_enabled() {
            return ProtocolDisplayResult::HexFallback(DisplayFallbackReason::NotDeclared);
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
        let entry =
            self.encode
                .clone()
                .ok_or_else(|| ProtocolRuntimeError::EntryPointUnavailable {
                    package: self.package.clone(),
                    direction: self.plan.direction(),
                    entry: ProtocolEntryPoint::Encode,
                })?;
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
        let entry = self
            .display
            .clone()
            .ok_or_else(|| ProtocolRuntimeError::EntryPointFailed {
                package: self.package.clone(),
                entry: ProtocolEntryPoint::Display,
            })?;
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

fn compiled_direction(
    package: &CompiledProtocolPackage,
    direction: ProtocolDirection,
) -> &CompiledDirection {
    match direction {
        ProtocolDirection::Upstream => package.upstream(),
        ProtocolDirection::Downstream => package.downstream(),
    }
}

fn exceeds_limit(length: usize, limit: u64) -> bool {
    u64::try_from(length).map_or(true, |length| length > limit)
}

fn validate_document_schema(
    document: &Document,
    expected: &intercept_proxy_domain::DocumentSchema,
) -> Result<(), ()> {
    if document.schema() != expected {
        return Err(());
    }
    // Document 的字段槽只能经类型安全 Domain API 或已校验反序列化创建；遍历仍在执行边界重新
    // 核对，避免未来新增构造路径时让错误字段类型进入规则或 Encode。
    if document.fields().any(|state| {
        state
            .value
            .is_some_and(|value| value.field_type() != state.field.field_type())
    }) {
        return Err(());
    }
    Ok(())
}

fn find_resource_limit(error: &EvalAltResult) -> Option<ProtocolResourceLimit> {
    match error {
        EvalAltResult::ErrorTooManyOperations(_) => Some(ProtocolResourceLimit::Operations),
        EvalAltResult::ErrorStackOverflow(_) => Some(ProtocolResourceLimit::CallDepth),
        EvalAltResult::ErrorDataTooLarge(kind, _)
            if kind.to_ascii_lowercase().contains("string") =>
        {
            Some(ProtocolResourceLimit::StringBytes)
        }
        EvalAltResult::ErrorDataTooLarge(_, _) => Some(ProtocolResourceLimit::BlobBytes),
        EvalAltResult::ErrorTerminated(_, _) => Some(ProtocolResourceLimit::WallTimeMs),
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _)
        | EvalAltResult::ErrorInModule(_, inner, _) => find_resource_limit(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
