//! 需求编号到应用层行为的回归测试。
//!
//! 这些测试不是示例代码，而是需求文档的可执行证据。测试名称和注释中的需求编号用于
//! 追踪 UI、Rust 用例和验收条件，修改业务语义时必须同步更新需求文档和对应测试。

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use intercept_proxy_domain::{
    DownstreamClientAuthentication, FixedServerSettings, UpstreamTlsSettings,
};
use intercept_proxy_product_api::{BodyCodec, ProductError};
use uuid::Uuid;

use crate::*;

mod support;

use support::*;

mod android_runtime;
mod breakpoints;
mod capacity;
mod diagnostics;
mod events;
mod listener_certificates;
mod protocol_package_lifecycle;
mod settings_lifecycle;
mod workspace_configuration;
