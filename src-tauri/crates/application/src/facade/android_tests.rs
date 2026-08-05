use super::*;

fn package(name: &str) -> AndroidPackageViewModel {
    AndroidPackageViewModel {
        package_name: name.into(),
        uid: 10_001,
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
        proxy_routes: Vec::new(),
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        weak_network: intercept_proxy_domain::WeakNetworkProfile::default(),
    };
    let packages = vec![
        AndroidPackageViewModel {
            package_name: "com.example.one".into(),
            uid: 10_042,
            shared_uid: Some(10_042),
        },
        AndroidPackageViewModel {
            package_name: "com.example.two".into(),
            uid: 10_042,
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
