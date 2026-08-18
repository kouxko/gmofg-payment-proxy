use std::{fmt, sync::Arc};

use crate::{
    BodyCodec, ProductCertificatePolicy, ProductChannel, ProductFaultTemplate, ProductLabels,
    ProductStorageNamespace, RequestClassifier,
};

/// 注入到 UI 无关 Rust Host 中的宿主配置总契约。
pub trait ProductProfile: fmt::Debug + Send + Sync {
    /// 稳定、机器可读的宿主 ID。
    fn id(&self) -> &'static str;

    /// 界面显示的应用名称。
    fn name(&self) -> &'static str;

    /// 静态监听通道及默认上游；动态 Workspace 可返回空切片。
    fn channels(&self) -> &'static [ProductChannel];

    /// 数据库与系统密钥存储的隔离命名空间。
    fn storage(&self) -> ProductStorageNamespace;

    fn labels(&self) -> ProductLabels;

    /// 返回空切片表示使用代理核心提供的完整通用故障目录。
    fn fault_templates(&self) -> &'static [ProductFaultTemplate];

    fn request_classifier(&self) -> Arc<dyn RequestClassifier>;

    fn certificates(&self) -> &dyn ProductCertificatePolicy;

    fn body_codec(&self) -> Arc<dyn BodyCodec>;
}
