//! 协议包 ZIP 导入与内置模板恢复用例。

use super::Application;
use crate::{
    AppResult, OperationResultViewModel, ProtocolPackageImportPreviewViewModel,
    ProtocolPackageImportToken, ProtocolPackageImportViewModel, UiTone,
};

impl Application {
    /// 通过宿主原生文件选择器导入 ZIP；WebView 不提交路径或文件内容。
    pub async fn protocol_package_import(
        &self,
    ) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>> {
        // 原生文件 Dialog 可能长时间等待用户选择，不能在这段交互期间占用全局 mutation_gate。
        // 注册表自身会串行化同一身份的 install/delete/cache 写入；导入又不会改写既有启用位或
        // Listener 引用，因此无需阻塞其他独立的 Application 变更。
        self.protocol_package_importer.prepare_zip().await
    }

    /// 使用 prepare 阶段返回的随机 token 原子提交被冻结的已验证包。
    pub async fn protocol_package_import_commit(
        &self,
        token: ProtocolPackageImportToken,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.protocol_package_importer.commit_zip(token).await
    }

    /// 关闭已就绪预览时立即释放 pending 容量；无效、过期或已使用 token 均稳定失败。
    pub async fn protocol_package_import_discard(
        &self,
        token: ProtocolPackageImportToken,
    ) -> AppResult<OperationResultViewModel> {
        self.protocol_package_importer.discard_zip(token).await?;
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "待确认的协议包导入已释放。".into(),
            ui_tone: UiTone::Neutral,
            entity_id: None,
            revision: None,
            requires_restart: false,
        })
    }

    /// 从应用内置的不可信 ZIP 重新恢复官方起始示例。
    pub async fn protocol_package_restore_builtin(
        &self,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.protocol_package_builtin.restore_builtin().await
    }

    /// 读取编译期内置的官方起始示例 ZIP，不访问或修改安装库。
    pub async fn protocol_package_builtin_archive(&self) -> AppResult<Vec<u8>> {
        self.protocol_package_builtin.builtin_archive().await
    }
}
