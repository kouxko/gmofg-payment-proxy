use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use intercept_proxy_domain::{
    DirectionProcessingOptions, Document, DocumentSchema, ProtocolPackageRef,
};

use crate::{
    CompiledProtocolPackage, LocalResponseOwnershipViolation, ProtocolDirection,
    ProtocolDirectionExecutor, ProtocolEntryPoint, ProtocolExecutionCancellation,
    ProtocolResourceLimit, ProtocolRuntimeError, ProtocolRuntimeLimits, ProtocolRuntimeResult,
};

use super::{DirectionExecutionPlan, ProtocolDisplayResult, ProtocolFrameOutput};

/// `LocalResponder` 已完成 request Decode 的只读桥接对象。
///
/// Decode 关闭时 [`Self::document`] 为 `None`。字段全部私有且只提供不可变借用，所以下游规则
/// 无法修改 Request Document；协调器会在 response 阶段显式 clone 或创建空 Document。
pub struct LocalRequestOutput {
    owner: Arc<()>,
    package: ProtocolPackageRef,
    schema: Arc<DocumentSchema>,
    connection_id: String,
    output: ProtocolFrameOutput,
    response_started: AtomicBool,
}

impl LocalRequestOutput {
    /// 返回 App 发来的完整 request Frame。
    #[must_use]
    pub fn origin(&self) -> &[u8] {
        self.output.origin()
    }

    /// Decode 开启时返回只读 Request Document；关闭时返回 `None`。
    #[must_use]
    pub const fn document(&self) -> Option<&Document> {
        self.output.decoded_document()
    }
}

impl fmt::Debug for LocalRequestOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRequestOutput")
            .field("package", &self.package)
            .field("connection_id", &self.connection_id)
            .field("origin_bytes", &self.origin().len())
            .field("decoded", &self.document().is_some())
            .finish_non_exhaustive()
    }
}

/// `LocalResponder` 已确定但尚未由调用方确认 write + flush 的单次 response。
pub struct LocalResponseOutput {
    owner: Arc<()>,
    package: ProtocolPackageRef,
    schema: Arc<DocumentSchema>,
    connection_id: String,
    output: Arc<ProtocolFrameOutput>,
    committed: AtomicBool,
}

impl LocalResponseOutput {
    /// 返回产生该 response 的原始 request Frame。
    #[must_use]
    pub fn request_origin(&self) -> &[u8] {
        self.output.origin()
    }

    /// 返回 downstream 规则完成后的独占 Response Document 快照。
    #[must_use]
    pub fn response_document(&self) -> &Document {
        self.output.execution_document()
    }

    /// 返回应该恰好写回 App 一次的非空 response 字节。
    #[must_use]
    pub fn written(&self) -> &[u8] {
        self.output.written()
    }

    /// 返回 response 字节的共享 owner，供异步 write 在不复制 payload 的前提下持有。
    #[must_use]
    pub fn written_owner(&self) -> Arc<[u8]> {
        self.output.written_owner()
    }
}

impl fmt::Debug for LocalResponseOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalResponseOutput")
            .field("package", &self.package)
            .field("connection_id", &self.connection_id)
            .field("request_bytes", &self.request_origin().len())
            .field("response_bytes", &self.written().len())
            .field("schema_id", &self.schema.id())
            .field("schema_version", &self.schema.version())
            .finish_non_exhaustive()
    }
}

/// 调用方确认 response 已完整 write + flush 后才能取得的 Display 句柄。
///
/// 句柄没有公开构造器，也不暴露线路字节；Display 只是提交后的 UI 旁路，失败只能降级 Hex。
pub struct LocalResponseDisplayHandle {
    owner: Arc<()>,
    package: ProtocolPackageRef,
    schema: Arc<DocumentSchema>,
    connection_id: String,
    output: Arc<ProtocolFrameOutput>,
}

impl fmt::Debug for LocalResponseDisplayHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalResponseDisplayHandle")
            .field("package", &self.package)
            .field("connection_id", &self.connection_id)
            .field("response_bytes", &self.output.written().len())
            .finish_non_exhaustive()
    }
}

/// 单连接、单协议包的 `LocalResponder` request/response 协调器。
///
/// 上游执行器固定禁止 Encode；下游执行器固定禁止 Decode。这样不存在的 Server 输入流不会被
/// 伪造成 downstream Frame/Decode，且同一个 exchange 只有 Request Document clone 这一条受控桥。
pub struct LocalResponderCoordinator {
    owner: Arc<()>,
    package: ProtocolPackageRef,
    schema: Arc<DocumentSchema>,
    connection_id: String,
    listener_id: String,
    upstream: ProtocolDirectionExecutor,
    downstream: ProtocolDirectionExecutor,
    limits: ProtocolRuntimeLimits,
    cancellation: ProtocolExecutionCancellation,
}

impl LocalResponderCoordinator {
    /// 使用独立取消句柄构造单连接协调器。
    pub fn new(
        package: &CompiledProtocolPackage,
        upstream_decode_enabled: bool,
        downstream_encode_enabled: bool,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
        limits: ProtocolRuntimeLimits,
    ) -> ProtocolRuntimeResult<Self> {
        Self::new_with_cancellation(
            package,
            upstream_decode_enabled,
            downstream_encode_enabled,
            connection_id,
            listener_id,
            limits,
            ProtocolExecutionCancellation::new(),
        )
    }

    /// 使用与 upstream Frame Inspector 共享的取消句柄构造单连接协调器。
    pub fn new_with_cancellation(
        package: &CompiledProtocolPackage,
        upstream_decode_enabled: bool,
        downstream_encode_enabled: bool,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
        limits: ProtocolRuntimeLimits,
        cancellation: ProtocolExecutionCancellation,
    ) -> ProtocolRuntimeResult<Self> {
        let connection_id = connection_id.into();
        let listener_id = listener_id.into();
        let upstream_plan = DirectionExecutionPlan::new(
            package,
            ProtocolDirection::Upstream,
            DirectionProcessingOptions {
                decode_enabled: upstream_decode_enabled,
                encode_enabled: false,
            },
        )?;
        let downstream_plan = DirectionExecutionPlan::new(
            package,
            ProtocolDirection::Downstream,
            DirectionProcessingOptions {
                decode_enabled: false,
                encode_enabled: downstream_encode_enabled,
            },
        )?;
        let upstream = ProtocolDirectionExecutor::new_with_cancellation(
            package,
            upstream_plan,
            connection_id.clone(),
            listener_id.clone(),
            limits,
            cancellation.clone(),
        )?;
        let downstream = ProtocolDirectionExecutor::new_with_cancellation(
            package,
            downstream_plan,
            connection_id.clone(),
            listener_id.clone(),
            limits,
            cancellation.clone(),
        )?;
        Ok(Self {
            owner: Arc::new(()),
            package: package.package().clone(),
            schema: package.schema_arc(),
            connection_id,
            listener_id,
            upstream,
            downstream,
            limits,
            cancellation,
        })
    }

    /// 返回可同时注入 upstream Frame Inspector 和规则连接的取消句柄。
    #[must_use]
    pub fn cancellation(&self) -> ProtocolExecutionCancellation {
        self.cancellation.clone()
    }

    /// 对一个已由 upstream Frame Inspector 切出的完整 request 执行可选 Decode。
    pub fn decode_request(&mut self, origin: Vec<u8>) -> ProtocolRuntimeResult<LocalRequestOutput> {
        let output = self.upstream.execute_frame(origin)?;
        Ok(LocalRequestOutput {
            owner: Arc::clone(&self.owner),
            package: self.package.clone(),
            schema: Arc::clone(&self.schema),
            connection_id: self.connection_id.clone(),
            output,
            response_started: AtomicBool::new(false),
        })
    }

    /// 从当前 request 构造 Response Document，执行一次 owned 规则变换并决定 Encode/Echo 输出。
    ///
    /// Decode 开启时初值是 Request Document 的独立 clone；关闭时是同 Schema 空 Document。
    /// `transform` 无论 Decode 是否开启都恰好调用一次，使空 Document 仍可执行无条件静态规则。
    pub fn build_response(
        &mut self,
        request: &LocalRequestOutput,
        transform: impl FnOnce(Document) -> ProtocolRuntimeResult<Document>,
    ) -> ProtocolRuntimeResult<LocalResponseOutput> {
        self.validate_request(request)?;
        self.ensure_not_cancelled()?;
        if request
            .response_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(self.ownership_error(LocalResponseOwnershipViolation::Output));
        }
        let initial = request
            .document()
            .cloned()
            .unwrap_or_else(|| Document::new(Arc::clone(&self.schema)));
        let document = transform(initial)?;
        self.ensure_not_cancelled()?;
        self.validate_schema(document.schema())?;
        let output = self
            .downstream
            .execute_predecoded_document(request.origin().to_vec(), document)?;
        if output.written().is_empty() {
            return Err(ProtocolRuntimeError::LocalResponseEmpty {
                package: self.package.clone(),
            });
        }
        if u64::try_from(output.written().len())
            .map_or(true, |length| length > self.limits.max_blob_bytes())
        {
            return Err(ProtocolRuntimeError::ResourceLimitExceeded {
                package: self.package.clone(),
                entry: ProtocolEntryPoint::Encode,
                limit: ProtocolResourceLimit::BlobBytes,
            });
        }
        Ok(LocalResponseOutput {
            owner: Arc::clone(&self.owner),
            package: self.package.clone(),
            schema: Arc::clone(&self.schema),
            connection_id: self.connection_id.clone(),
            output: Arc::new(output),
            committed: AtomicBool::new(false),
        })
    }

    /// 在调用方完整写入并 flush response 后，把输出升级为可展示句柄。
    pub fn response_committed(
        &self,
        response: &LocalResponseOutput,
    ) -> ProtocolRuntimeResult<LocalResponseDisplayHandle> {
        self.validate_response(response)?;
        if response
            .committed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(self.ownership_error(LocalResponseOwnershipViolation::Output));
        }
        Ok(LocalResponseDisplayHandle {
            owner: Arc::clone(&response.owner),
            package: response.package.clone(),
            schema: Arc::clone(&response.schema),
            connection_id: response.connection_id.clone(),
            output: Arc::clone(&response.output),
        })
    }

    /// 对成功 Decode 的 request 执行公共 Display；失败只返回 Hex 回退。
    pub fn render_request_display(
        &mut self,
        request: &LocalRequestOutput,
    ) -> ProtocolRuntimeResult<ProtocolDisplayResult> {
        self.validate_request(request)?;
        Ok(self.upstream.render_decoded_display(&request.output))
    }

    /// 对已提交 response 执行可选 downstream Display；脚本失败只返回 Hex 回退。
    pub fn render_response_display(
        &mut self,
        handle: &LocalResponseDisplayHandle,
    ) -> ProtocolRuntimeResult<ProtocolDisplayResult> {
        self.validate_display_handle(handle)?;
        Ok(self
            .downstream
            .render_output_document_display(&handle.output))
    }

    fn validate_request(&self, request: &LocalRequestOutput) -> ProtocolRuntimeResult<()> {
        self.validate_identity(
            &request.owner,
            &request.package,
            &request.schema,
            &request.connection_id,
        )?;
        if let Some(document) = request.document() {
            self.validate_schema(document.schema())?;
        }
        Ok(())
    }

    fn validate_response(&self, response: &LocalResponseOutput) -> ProtocolRuntimeResult<()> {
        self.validate_identity(
            &response.owner,
            &response.package,
            &response.schema,
            &response.connection_id,
        )?;
        self.validate_schema(response.response_document().schema())
    }

    fn validate_display_handle(
        &self,
        handle: &LocalResponseDisplayHandle,
    ) -> ProtocolRuntimeResult<()> {
        self.validate_identity(
            &handle.owner,
            &handle.package,
            &handle.schema,
            &handle.connection_id,
        )?;
        self.validate_schema(handle.output.execution_document().schema())
    }

    fn validate_identity(
        &self,
        owner: &Arc<()>,
        package: &ProtocolPackageRef,
        schema: &DocumentSchema,
        connection_id: &str,
    ) -> ProtocolRuntimeResult<()> {
        if package != &self.package {
            return Err(self.ownership_error(LocalResponseOwnershipViolation::Package));
        }
        self.validate_schema(schema)?;
        if connection_id != self.connection_id {
            return Err(self.ownership_error(LocalResponseOwnershipViolation::Connection));
        }
        if !Arc::ptr_eq(owner, &self.owner) {
            return Err(self.ownership_error(LocalResponseOwnershipViolation::Output));
        }
        Ok(())
    }

    fn validate_schema(&self, schema: &DocumentSchema) -> ProtocolRuntimeResult<()> {
        if schema == self.schema.as_ref() {
            Ok(())
        } else {
            Err(self.ownership_error(LocalResponseOwnershipViolation::Schema))
        }
    }

    fn ownership_error(&self, violation: LocalResponseOwnershipViolation) -> ProtocolRuntimeError {
        ProtocolRuntimeError::LocalResponseOwnershipViolation {
            package: self.package.clone(),
            violation,
        }
    }

    fn ensure_not_cancelled(&self) -> ProtocolRuntimeResult<()> {
        if self.cancellation.is_cancelled() {
            Err(ProtocolRuntimeError::LocalResponseCancelled {
                package: self.package.clone(),
            })
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for LocalResponderCoordinator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalResponderCoordinator")
            .field("package", &self.package)
            .field("schema_id", &self.schema.id())
            .field("schema_version", &self.schema.version())
            .field("connection_id", &self.connection_id)
            .field("listener_id", &self.listener_id)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}
