//! 内部与外部协议包注册表的跨来源身份查询。

use intercept_proxy_domain::ProtocolPackageRef;
use rusqlite::{Transaction, params};

use super::{InfrastructureError, database_error};

/// 判断精确身份是否已由外部执行来源占用。
///
/// 内部与外部注册表必须共同维护跨来源唯一性；此检查必须在内部安装的写事务中执行，
/// 不能依赖稍后的统一目录合并，否则两个来源会同时拥有同一 Listener 绑定身份。
pub(super) fn exact_external_package_exists(
    transaction: &Transaction<'_>,
    package: &ProtocolPackageRef,
) -> Result<bool, InfrastructureError> {
    transaction
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM external_protocol_packages
                WHERE package_id = ?1 AND version = ?2
             )",
            params![package.id.as_str(), package.version.as_str()],
            |row| row.get(0),
        )
        .map_err(database_error)
}
