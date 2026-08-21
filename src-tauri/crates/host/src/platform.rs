//! Host 平台文件系统与秘密保护器选择。

use std::{path::Path, sync::Arc};

#[cfg(not(target_os = "macos"))]
use intercept_proxy_infrastructure::DpapiProtector;
#[cfg(target_os = "macos")]
use intercept_proxy_infrastructure::MacKeychainProtector;
use intercept_proxy_infrastructure::SecretProtector;
use intercept_proxy_product_api::ProductStorageNamespace;

use crate::HostBuildError;

pub(super) fn create_data_directory(path: &Path) -> Result<(), HostBuildError> {
    std::fs::create_dir_all(path).map_err(|source| HostBuildError::CreateDataDirectory {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn platform_secret_protector(
    storage: ProductStorageNamespace,
) -> Arc<dyn SecretProtector> {
    #[cfg(windows)]
    {
        let _ = storage;
        Arc::new(DpapiProtector)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacKeychainProtector::for_namespace(storage))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = storage;
        Arc::new(DpapiProtector)
    }
}
