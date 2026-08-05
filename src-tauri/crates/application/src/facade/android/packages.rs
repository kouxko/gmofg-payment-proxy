use super::{
    AndroidAdbViewModel, AndroidDeviceViewModel, AndroidNetworkProfile, AndroidPackageViewModel,
    AndroidTargetApplication, AppError, AppResult, Application, BTreeSet,
};

impl Application {
    pub async fn android_adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        self.android.adb_get().await
    }

    pub async fn android_adb_select(&self, serial: String) -> AppResult<AndroidAdbViewModel> {
        validate_serial(&serial)?;
        let _gate = self.mutation_gate.lock().await;
        let selected = self.android.adb_select(serial).await?;
        // 即便用户重新选择同一设备，也视为显式刷新包清单。这样安装、卸载或升级
        // 应用后不需要重启桌面端，同时包名筛选仍可复用包清单结果。
        *self.android_package_cache.lock().await = None;
        Ok(selected)
    }

    pub async fn android_device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        self.android.device_list().await
    }

    pub async fn android_package_list(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        let mut cache = self.android_package_cache.lock().await;
        if let Some(packages) = cache.as_ref() {
            return Ok(packages.clone());
        }
        let mut packages = self.android.package_list().await?;
        packages.retain(|package| package.package_name != crate::ANDROID_COMPANION_PACKAGE);
        *cache = Some(packages.clone());
        Ok(packages)
    }

    /// 丢弃当前设备的包清单缓存并重新读取设备。
    ///
    /// APK 安装、卸载或升级不会主动通知桌面进程，因此所有宿主（桌面 UI、未来
    /// CLI/TUI 和无界面测试）都通过该用例获得一致的显式刷新语义。
    pub async fn android_package_refresh(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        self.refresh_android_package_inventory().await
    }

    /// 包名筛选由 Rust 完成，前端只提交用户输入并渲染返回结果。
    /// 空关键字等价于完整列表；比较时忽略 ASCII 大小写。
    pub async fn android_package_query(
        &self,
        query: String,
    ) -> AppResult<Vec<AndroidPackageViewModel>> {
        let packages = self.android_package_list().await?;
        filter_packages(packages, &query)
    }

    pub async fn android_package_get(
        &self,
        package_name: String,
    ) -> AppResult<AndroidPackageViewModel> {
        validate_package_name(&package_name)?;
        self.android.package_get(package_name).await
    }

    pub(super) async fn refresh_android_package_inventory(
        &self,
    ) -> AppResult<Vec<AndroidPackageViewModel>> {
        let mut packages = self.android.package_list().await?;
        packages.retain(|package| package.package_name != crate::ANDROID_COMPANION_PACKAGE);
        *self.android_package_cache.lock().await = Some(packages.clone());
        Ok(packages)
    }
}

pub(super) fn filter_packages(
    packages: Vec<AndroidPackageViewModel>,
    query: &str,
) -> AppResult<Vec<AndroidPackageViewModel>> {
    if query.chars().count() > 255 {
        return Err(AppError::new(
            "ANDROID_PACKAGE_QUERY_TOO_LONG",
            "包名筛选关键字不能超过 255 个字符。",
        ));
    }
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Ok(packages);
    }
    Ok(packages
        .into_iter()
        .filter(|package| package.package_name.to_ascii_lowercase().contains(&query))
        .collect())
}

pub(super) fn apply_package_toggle(
    profile: &mut AndroidNetworkProfile,
    packages: &[AndroidPackageViewModel],
    package_name: &str,
    selected: bool,
) -> AppResult<()> {
    let selected_package = packages
        .iter()
        .find(|package| package.package_name == package_name)
        .ok_or_else(|| {
            AppError::new(
                "ANDROID_TARGET_PACKAGE_CHANGED",
                format!("目标应用 {package_name} 已卸载。"),
            )
        })?;
    if selected_package.package_name == crate::ANDROID_COMPANION_PACKAGE {
        return Err(AppError::new(
            "ANDROID_COMPANION_CANNOT_BE_TARGETED",
            "设备端组件自身不能进入网络接管允许列表。",
        ));
    }

    let group = packages
        .iter()
        .filter(|candidate| match selected_package.shared_uid {
            Some(shared_uid) => candidate.uid == shared_uid,
            None => candidate.package_name == selected_package.package_name,
        })
        .collect::<Vec<_>>();
    let group_names = group
        .iter()
        .map(|package| package.package_name.as_str())
        .collect::<BTreeSet<_>>();
    profile
        .target_applications
        .retain(|target| !group_names.contains(target.package_name.as_str()));

    if let Some(shared_uid) = selected_package.shared_uid {
        profile.confirmed_shared_uids.remove(&shared_uid);
        if selected {
            profile.confirmed_shared_uids.insert(shared_uid);
        }
    }
    if selected {
        let targets = group
            .into_iter()
            .map(target_from_installed_package)
            .collect::<Vec<_>>();
        profile.target_applications.extend(targets);
    }
    profile
        .target_applications
        .sort_by(|left, right| left.package_name.cmp(&right.package_name));
    Ok(())
}

pub(super) fn target_from_installed_package(
    package: &AndroidPackageViewModel,
) -> AndroidTargetApplication {
    AndroidTargetApplication {
        package_name: package.package_name.clone(),
        uid: package.uid,
        display_name: Some(package.package_name.clone()),
    }
}

pub(super) fn validate_serial(serial: &str) -> AppResult<()> {
    if serial.is_empty()
        || serial.len() > 128
        || !serial
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
    {
        return Err(AppError::new(
            "ANDROID_SERIAL_INVALID",
            "安卓设备序列号格式无效。",
        ));
    }
    Ok(())
}

pub(super) fn validate_profile_id(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(AppError::new(
            "ANDROID_PROFILE_ID_INVALID",
            "设备网络方案 ID 格式无效。",
        ));
    }
    Ok(())
}

pub(super) fn validate_package_name(value: &str) -> AppResult<()> {
    if value.len() > 255
        || !value.contains('.')
        || !value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
    {
        return Err(AppError::new(
            "ANDROID_PACKAGE_NAME_INVALID",
            "Android 包名格式无效。",
        ));
    }
    Ok(())
}
