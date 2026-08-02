use crate::{
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidDeviceViewModel,
    AndroidNetworkProfile, AndroidNetworkProfileSummary, AndroidNetworkStatusViewModel,
    AndroidPackageViewModel, AndroidProfileEditIntent, AndroidTargetApplication, AppError,
    AppResult, OperationResultViewModel,
};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use super::Application;

impl Application {
    pub async fn android_adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        self.android.adb_get().await
    }

    pub async fn android_adb_select(&self, serial: String) -> AppResult<AndroidAdbViewModel> {
        validate_serial(&serial)?;
        let selected = self.android.adb_select(serial).await?;
        // 即便用户重新选择同一设备，也视为显式刷新包清单。这样安装、卸载或升级
        // 应用后不需要重启桌面端，同时包名筛选仍可复用昂贵的签名读取结果。
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

    pub async fn android_companion_install(&self) -> AppResult<AndroidCompanionInstallViewModel> {
        self.android.companion_install(false).await
    }

    pub async fn android_companion_update(&self) -> AppResult<AndroidCompanionInstallViewModel> {
        self.android.companion_install(true).await
    }

    pub async fn android_vpn_open_consent(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        self.android.vpn_open_consent().await
    }

    pub async fn device_network_profile_list(
        &self,
    ) -> AppResult<Vec<AndroidNetworkProfileSummary>> {
        self.android.profile_list().await
    }

    /// 由 Rust 生成稳定方案 ID 和完整弱网默认值；展示层不得自行构造领域对象。
    pub fn device_network_profile_new(&self) -> AndroidNetworkProfile {
        AndroidNetworkProfile {
            id: Uuid::new_v4().to_string(),
            name: "新建弱网方案".into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            confirmed_shared_uids: BTreeSet::default(),
            auto_resume_after_reboot: false,
            weak_network: intercept_proxy_domain::WeakNetworkProfile::default(),
        }
    }

    pub async fn device_network_profile_get(
        &self,
        profile_id: String,
    ) -> AppResult<AndroidNetworkProfile> {
        validate_profile_id(&profile_id)?;
        self.android.profile_get(profile_id).await
    }

    /// 将页面编辑意图规范化为完整 Profile。
    ///
    /// 选择共享 UID 应用时，Rust 自动选择整个 UID 组，并把这次明确点击记录为整组确认；
    /// 取消时整组移除。前端不接触签名快照、UID 分组或嵌套默认值。
    pub async fn device_network_profile_apply_intent(
        &self,
        mut profile: AndroidNetworkProfile,
        intent: AndroidProfileEditIntent,
    ) -> AppResult<AndroidNetworkProfile> {
        if let AndroidProfileEditIntent::TogglePackage {
            package_name,
            selected,
        } = &intent
        {
            validate_package_name(package_name)?;
            let packages = self.android_package_list().await?;
            apply_package_toggle(&mut profile, &packages, package_name, *selected)?;
        } else {
            intent.apply_defaults(&mut profile);
        }
        Ok(profile)
    }

    pub async fn device_network_profile_save(
        &self,
        profile: AndroidNetworkProfile,
    ) -> AppResult<AndroidNetworkProfile> {
        profile.validate()?;
        self.validate_profile_against_device(&profile).await?;
        self.android.profile_save(profile).await
    }

    pub async fn device_network_profile_delete(
        &self,
        profile_id: String,
    ) -> AppResult<OperationResultViewModel> {
        validate_profile_id(&profile_id)?;
        self.android.profile_delete(profile_id).await
    }

    pub async fn device_network_start(
        &self,
        profile_id: String,
        dangerous_confirmed: bool,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.validate_network_activation(&profile_id, dangerous_confirmed)
            .await?;
        self.android.network_start(profile_id).await
    }

    pub async fn device_network_apply(
        &self,
        profile_id: String,
        dangerous_confirmed: bool,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.validate_network_activation(&profile_id, dangerous_confirmed)
            .await?;
        self.android.network_apply(profile_id).await
    }

    pub async fn device_network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        self.android.network_stop().await
    }

    pub async fn device_network_emergency_restore(
        &self,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.android.emergency_restore().await
    }

    pub async fn device_network_status(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        self.android.network_status().await
    }

    async fn validate_network_activation(
        &self,
        profile_id: &str,
        confirmed: bool,
    ) -> AppResult<()> {
        validate_profile_id(profile_id)?;
        let profile = self.android.profile_get(profile_id.to_owned()).await?;
        profile.validate()?;
        self.validate_profile_against_device(&profile).await?;
        if profile.requires_dangerous_confirmation() && !confirmed {
            return Err(AppError::new(
                "ANDROID_DANGEROUS_CONFIRMATION_REQUIRED",
                "100% 丢包或黑洞窗口需要显式二次确认。",
            ));
        }
        Ok(())
    }

    async fn validate_profile_against_device(
        &self,
        profile: &AndroidNetworkProfile,
    ) -> AppResult<()> {
        let packages = self.android_package_list().await?;
        let inventory = packages
            .iter()
            .map(|package| (package.package_name.as_str(), package))
            .collect::<BTreeMap<_, _>>();
        let selected = profile
            .target_applications
            .iter()
            .map(|target| target.package_name.as_str())
            .collect::<BTreeSet<_>>();

        for target in &profile.target_applications {
            let installed = inventory.get(target.package_name.as_str()).ok_or_else(|| {
                AppError::new(
                    "ANDROID_TARGET_PACKAGE_CHANGED",
                    format!("目标应用 {} 已卸载。", target.package_name),
                )
            })?;
            if installed.uid != target.uid
                || installed.signing_sha256.as_deref() != Some(target.signing_sha256.as_str())
            {
                return Err(AppError::new(
                    "ANDROID_TARGET_PACKAGE_CHANGED",
                    format!("目标应用 {} 的 UID 或签名已变化。", target.package_name),
                ));
            }
            if let Some(shared_uid) = installed.shared_uid {
                let complete_group = packages
                    .iter()
                    .filter(|package| package.uid == shared_uid)
                    .map(|package| package.package_name.as_str())
                    .collect::<BTreeSet<_>>();
                if !complete_group.is_subset(&selected) {
                    return Err(AppError::new(
                        "ANDROID_SHARED_UID_PARTIAL_SELECTION",
                        format!("UID {shared_uid} 的共享应用组必须完整选择。"),
                    ));
                }
                if !profile.confirmed_shared_uids.contains(&shared_uid) {
                    return Err(AppError::new(
                        "ANDROID_SHARED_UID_CONFIRMATION_REQUIRED",
                        format!("共享 UID {shared_uid} 需要显式确认。"),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn filter_packages(
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

fn apply_package_toggle(
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
            .collect::<AppResult<Vec<_>>>()?;
        profile.target_applications.extend(targets);
    }
    profile
        .target_applications
        .sort_by(|left, right| left.package_name.cmp(&right.package_name));
    Ok(())
}

fn target_from_installed_package(
    package: &AndroidPackageViewModel,
) -> AppResult<AndroidTargetApplication> {
    let signing_sha256 = package.signing_sha256.clone().ok_or_else(|| {
        AppError::new(
            "ANDROID_TARGET_SIGNATURE_UNAVAILABLE",
            format!("无法读取目标应用 {} 的签名。", package.package_name),
        )
    })?;
    Ok(AndroidTargetApplication {
        package_name: package.package_name.clone(),
        signing_sha256,
        uid: package.uid,
        display_name: Some(package.package_name.clone()),
    })
}

fn validate_serial(serial: &str) -> AppResult<()> {
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

fn validate_profile_id(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(AppError::new(
            "ANDROID_PROFILE_ID_INVALID",
            "弱网方案 ID 格式无效。",
        ));
    }
    Ok(())
}

fn validate_package_name(value: &str) -> AppResult<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str) -> AndroidPackageViewModel {
        AndroidPackageViewModel {
            package_name: name.into(),
            uid: 10_001,
            signing_sha256: Some("AA".into()),
            shared_uid: None,
        }
    }

    #[test]
    fn package_query_filters_by_name_case_insensitively() {
        let result = filter_packages(
            vec![
                package("com.example.Payment"),
                package("com.example.launcher"),
            ],
            " payment ",
        )
        .expect("包名筛选应成功");

        assert_eq!(result, vec![package("com.example.Payment")]);
    }

    #[test]
    fn package_query_rejects_unbounded_input() {
        let error = filter_packages(vec![package("com.example.payment")], &"a".repeat(256))
            .expect_err("过长关键字必须由 Rust 拒绝");

        assert_eq!(error.view_model.code, "ANDROID_PACKAGE_QUERY_TOO_LONG");
    }

    #[test]
    fn package_toggle_expands_and_confirms_shared_uid_in_rust() {
        let mut profile = AndroidNetworkProfile {
            id: "shared".into(),
            name: "Shared".into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: intercept_proxy_domain::WeakNetworkProfile::default(),
        };
        let packages = vec![
            AndroidPackageViewModel {
                package_name: "com.example.one".into(),
                uid: 10_042,
                signing_sha256: Some("AA".repeat(32)),
                shared_uid: Some(10_042),
            },
            AndroidPackageViewModel {
                package_name: "com.example.two".into(),
                uid: 10_042,
                signing_sha256: Some("BB".repeat(32)),
                shared_uid: Some(10_042),
            },
        ];

        apply_package_toggle(&mut profile, &packages, "com.example.one", true)
            .expect("共享 UID 应整组扩选");

        assert_eq!(profile.target_applications.len(), 2);
        assert!(profile.confirmed_shared_uids.contains(&10_042));
        apply_package_toggle(&mut profile, &packages, "com.example.two", false)
            .expect("取消任一成员应移除整组");
        assert!(profile.target_applications.is_empty());
        assert!(profile.confirmed_shared_uids.is_empty());
    }
}
