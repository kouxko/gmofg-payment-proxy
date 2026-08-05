use super::{AppError, AppResult, Application, ChannelPresentationViewModel};

impl Application {
    /// 返回当前 Workspace 可供规则、故障和筛选使用的真实 Listener 目录。
    ///
    /// 这是动态代理配置与旧产品通道之间的唯一适配点。调用方不得从全局设置或模板
    /// 占位值推断通道，否则会保存一条永远无法匹配运行时 Listener 的规则。
    pub(crate) async fn selected_workspace_channel_catalog(
        &self,
    ) -> AppResult<Vec<ChannelPresentationViewModel>> {
        let selected_workspace = self
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|workspace| workspace.selected);
        let Some(summary) = selected_workspace else {
            return Ok(Vec::new());
        };
        self.workspaces
            .get(summary.id)
            .await?
            .listeners
            .into_iter()
            .map(|listener| {
                let id = listener.id;
                let display_name = listener.name;
                Ok(ChannelPresentationViewModel {
                    id: crate::ChannelId::new(id.to_string()).map_err(AppError::from)?,
                    display_name,
                })
            })
            .collect()
    }
}
