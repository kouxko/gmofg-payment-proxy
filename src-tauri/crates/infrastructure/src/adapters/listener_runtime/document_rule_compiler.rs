//! Bounded CPU owner for protocol document-rule compilation.

use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};

#[derive(Clone, Debug)]
pub(super) struct DocumentRuleCompiler {
    gate: Arc<tokio::sync::Semaphore>,
}

impl DocumentRuleCompiler {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            gate: Arc::new(tokio::sync::Semaphore::new(capacity)),
        }
    }

    pub(super) async fn compile<T, F>(&self, compile: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce() -> AppResult<T> + Send + 'static,
    {
        let permit = self.gate.clone().acquire_owned().await.map_err(|_| {
            AppError::new(
                "DOCUMENT_RULE_COMPILER_CLOSED",
                "协议报文规则编译服务已关闭。",
            )
        })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            compile()
        })
        .await
        .map_err(|error| {
            AppError::new(
                "DOCUMENT_RULE_COMPILER_PANICKED",
                format!("协议报文规则编译任务异常终止：{error}"),
            )
        })?
    }

    #[cfg(test)]
    pub(super) async fn occupy_all(&self) -> tokio::sync::OwnedSemaphorePermit {
        self.gate.clone().acquire_many_owned(4).await.unwrap()
    }
}
