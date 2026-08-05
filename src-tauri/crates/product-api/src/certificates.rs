use std::fmt;

/// 通用证书适配器展示的宿主自定义文案。
#[derive(Debug, Clone, Copy)]
pub struct CertificateLabels {
    pub root_name: &'static str,
    pub root_usage: &'static str,
    pub leaf_name: &'static str,
    pub leaf_usage: &'static str,
    pub client_identity_name: &'static str,
    pub client_identity_usage: &'static str,
    pub upstream_name: &'static str,
    pub upstream_bundled_usage: &'static str,
    pub upstream_override_usage: &'static str,
    pub ready_status: &'static str,
    pub incomplete_status: &'static str,
    pub already_exists_message: &'static str,
    pub export_cancelled_message: &'static str,
    pub export_success_message: &'static str,
}

/// 外层装配选择的证书展示文案与可选上游信任锚。
pub trait ProductCertificatePolicy: fmt::Debug + Send + Sync {
    /// 应用随包携带的默认上游信任锚，可由用户导入文件替换。
    fn bundled_upstream_ca_pem(&self) -> Option<&'static [u8]>;

    /// 固定运行模式可把共享上游客户端身份视为全局启动前置项。
    /// 通用 Intercept Proxy 按入口引用身份，因此默认不要求。
    fn requires_global_client_identity(&self) -> bool {
        false
    }

    /// 固定运行模式可把上游 CA 视为全局启动前置项。
    /// 通用 Intercept Proxy 按入口选择系统信任或 CA 引用，因此默认不要求。
    fn requires_global_upstream_ca(&self) -> bool {
        false
    }

    fn labels(&self) -> CertificateLabels;
}
