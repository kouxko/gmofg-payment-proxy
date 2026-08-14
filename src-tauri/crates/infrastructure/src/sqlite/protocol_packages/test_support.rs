//! 仅供协议包持久化测试构造损坏数据库状态的辅助入口。

use rusqlite::params;

use super::*;

impl SqliteStore {
    pub(crate) fn protocol_package_row_counts_for_test(&self) -> (i64, i64) {
        let connection = self.connection.lock();
        let packages = connection
            .query_row("SELECT COUNT(*) FROM protocol_packages", [], |row| {
                row.get(0)
            })
            .expect("protocol package row count");
        let files = connection
            .query_row("SELECT COUNT(*) FROM protocol_package_files", [], |row| {
                row.get(0)
            })
            .expect("protocol package file row count");
        (packages, files)
    }

    pub(crate) fn delete_protocol_package_file_for_test(
        &self,
        package: &ProtocolPackageRef,
        path: &str,
    ) {
        self.connection
            .lock()
            .execute(
                "DELETE FROM protocol_package_files
                 WHERE package_id = ?1 AND version = ?2 AND path = ?3",
                params![package.id.as_str(), package.version.as_str(), path],
            )
            .expect("delete protocol package test file");
    }

    pub(crate) fn replace_protocol_package_file_for_test(
        &self,
        package: &ProtocolPackageRef,
        path: &str,
        contents: &[u8],
    ) {
        self.connection
            .lock()
            .execute(
                "UPDATE protocol_package_files SET contents = ?4
                 WHERE package_id = ?1 AND version = ?2 AND path = ?3",
                params![
                    package.id.as_str(),
                    package.version.as_str(),
                    path,
                    contents,
                ],
            )
            .expect("replace protocol package test file");
    }

    pub(crate) fn rename_protocol_package_file_for_test(
        &self,
        package: &ProtocolPackageRef,
        old_path: &str,
        new_path: &str,
    ) {
        self.connection
            .lock()
            .execute(
                "UPDATE protocol_package_files SET path = ?4
                 WHERE package_id = ?1 AND version = ?2 AND path = ?3",
                params![
                    package.id.as_str(),
                    package.version.as_str(),
                    old_path,
                    new_path,
                ],
            )
            .expect("rename protocol package test file");
    }

    pub(crate) fn rename_protocol_package_for_test(
        &self,
        package: &ProtocolPackageRef,
        name: &str,
    ) {
        self.connection
            .lock()
            .execute(
                "UPDATE protocol_packages SET name = ?3
                 WHERE package_id = ?1 AND version = ?2",
                params![package.id.as_str(), package.version.as_str(), name],
            )
            .expect("rename protocol package test header");
    }

    pub(crate) fn corrupt_protocol_package_host_api_for_test(
        &self,
        package: &ProtocolPackageRef,
        host_api: i64,
    ) {
        self.connection
            .lock()
            .execute(
                "UPDATE protocol_packages SET host_api = ?3
                 WHERE package_id = ?1 AND version = ?2",
                params![package.id.as_str(), package.version.as_str(), host_api],
            )
            .expect("corrupt protocol package host API for recovery test");
    }

    pub(crate) fn replace_protocol_package_file_with_zeroblob_for_test(
        &self,
        package: &ProtocolPackageRef,
        path: &str,
        bytes: i64,
    ) {
        self.connection
            .lock()
            .execute(
                "UPDATE protocol_package_files SET contents = zeroblob(?4)
                 WHERE package_id = ?1 AND version = ?2 AND path = ?3",
                params![package.id.as_str(), package.version.as_str(), path, bytes],
            )
            .expect("replace protocol package test file with zero blob");
    }

    pub(crate) fn reject_protocol_package_file_for_test(&self, path: &str) {
        self.connection
            .lock()
            .execute_batch(&format!(
                "CREATE TRIGGER reject_protocol_package_file
                 BEFORE INSERT ON protocol_package_files
                 WHEN NEW.path = '{}'
                 BEGIN SELECT RAISE(ABORT, 'rejected test protocol file'); END;",
                path.replace('\'', "''")
            ))
            .expect("create protocol package rejection trigger");
    }
}
