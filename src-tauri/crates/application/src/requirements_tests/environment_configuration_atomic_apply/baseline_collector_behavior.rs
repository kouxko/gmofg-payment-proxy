use uuid::Uuid;

use crate::{
    EnvironmentAndroidOwnerBaseline, EnvironmentApplyGenerations, EnvironmentExactPackageBaseline,
    EnvironmentMaterialInventoryBaseline, EnvironmentValidatedApplyBaselineCollector,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

fn collect(
    workspace_id: Uuid,
    generations: EnvironmentApplyGenerations,
    structural_hash: [u8; 32],
) -> crate::AppResult<crate::EnvironmentValidatedApplyBaseline> {
    EnvironmentValidatedApplyBaselineCollector::collect(
        workspace_id,
        generations,
        structural_hash,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

fn complete_generations() -> EnvironmentApplyGenerations {
    EnvironmentApplyGenerations {
        selected_workspace_id: Some(Uuid::from_u128(0x38)),
        listener: 1,
        android: 2,
        package: 3,
        package_inventory: 4,
        certificate_inventory: 5,
        protected_secret_inventory: 6,
        application_mutation: 7,
    }
}

fn material_inventory() -> Vec<EnvironmentMaterialInventoryBaseline> {
    vec![EnvironmentMaterialInventoryBaseline::observed(
        "certificate:g038".into(),
        [0x38; 32],
    )]
}

#[test]
fn android_owner_baseline_is_sorted_by_profile_and_original_serial() {
    let baseline = EnvironmentValidatedApplyBaselineCollector::collect(
        Uuid::from_u128(0x38),
        complete_generations(),
        [1; 32],
        Vec::new(),
        vec![
            EnvironmentAndroidOwnerBaseline::observed(
                "profile-b".into(),
                "DEVICE-02".into(),
                Uuid::from_u128(2),
                "active".into(),
            ),
            EnvironmentAndroidOwnerBaseline::observed(
                "profile-a".into(),
                "DEVICE-10".into(),
                Uuid::from_u128(1),
                "waiting_reconnect".into(),
            ),
        ],
        Vec::new(),
        material_inventory(),
    )
    .unwrap();

    assert_eq!(
        baseline
            .android_owners()
            .iter()
            .map(|owner| (owner.profile_id(), owner.serial()))
            .collect::<Vec<_>>(),
        vec![("profile-a", "DEVICE-10"), ("profile-b", "DEVICE-02")]
    );
}

#[test]
fn android_owner_baseline_rejects_duplicate_profile_and_serial_key() {
    let duplicate = || {
        EnvironmentAndroidOwnerBaseline::observed(
            "profile-a".into(),
            "DEVICE-01".into(),
            Uuid::new_v4(),
            "active".into(),
        )
    };

    let result = EnvironmentValidatedApplyBaselineCollector::collect(
        Uuid::from_u128(0x38),
        complete_generations(),
        [1; 32],
        Vec::new(),
        vec![duplicate(), duplicate()],
        Vec::new(),
        material_inventory(),
    );

    assert!(result.is_err());
}

#[test]
fn collector_rejects_nil_workspace_identity() {
    let result = collect(
        Uuid::nil(),
        EnvironmentApplyGenerations {
            application_mutation: 1,
            ..EnvironmentApplyGenerations::default()
        },
        [1; 32],
    );

    assert!(result.is_err());
}

#[test]
fn collector_rejects_default_generations() {
    let result = collect(
        Uuid::from_u128(0x38),
        EnvironmentApplyGenerations::default(),
        [1; 32],
    );

    assert!(result.is_err());
}

#[test]
fn collector_rejects_zero_workspace_structural_hash() {
    let result = collect(
        Uuid::from_u128(0x38),
        EnvironmentApplyGenerations {
            application_mutation: 1,
            ..EnvironmentApplyGenerations::default()
        },
        [0; 32],
    );

    assert!(result.is_err());
}

#[test]
fn collector_rejects_an_empty_material_inventory() {
    let result = collect(
        Uuid::from_u128(0x38),
        EnvironmentApplyGenerations {
            selected_workspace_id: Some(Uuid::from_u128(0x38)),
            listener: 1,
            android: 2,
            package: 3,
            package_inventory: 4,
            certificate_inventory: 5,
            protected_secret_inventory: 6,
            application_mutation: 7,
        },
        [1; 32],
    );

    assert!(result.is_err());
}

#[test]
fn sealed_baseline_carries_candidate_schema_and_validation_engine_versions() {
    let baseline = EnvironmentValidatedApplyBaselineCollector::collect(
        Uuid::from_u128(0x38),
        EnvironmentApplyGenerations {
            selected_workspace_id: Some(Uuid::from_u128(0x38)),
            listener: 1,
            android: 2,
            package: 3,
            package_inventory: 4,
            certificate_inventory: 5,
            protected_secret_inventory: 6,
            application_mutation: 7,
        },
        [1; 32],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![EnvironmentMaterialInventoryBaseline::observed(
            "certificate:g038".into(),
            [0x38; 32],
        )],
    )
    .expect("complete baseline seals");

    assert_eq!(baseline.candidate_schema_version(), 1);
    assert_eq!(
        baseline.validation_engine_version(),
        crate::ENVIRONMENT_VALIDATION_ENGINE_VERSION
    );
}

fn package(version: &str, generation: u128) -> EnvironmentExactPackageBaseline {
    EnvironmentExactPackageBaseline::observed_projection(
        ProtocolPackageRef {
            id: ProtocolPackageId::new("pkg").unwrap(),
            version: ProtocolPackageVersion::new(version).unwrap(),
        },
        Uuid::from_u128(generation),
        true,
        true,
        1,
        [1; 32],
        1,
        1,
    )
}

#[test]
fn exact_package_baseline_uses_semver_canonical_order() {
    let baseline = EnvironmentValidatedApplyBaselineCollector::collect(
        Uuid::from_u128(0x38),
        EnvironmentApplyGenerations {
            selected_workspace_id: Some(Uuid::from_u128(0x38)),
            listener: 1,
            android: 2,
            package: 3,
            package_inventory: 4,
            certificate_inventory: 5,
            protected_secret_inventory: 6,
            application_mutation: 7,
        },
        [1; 32],
        Vec::new(),
        Vec::new(),
        vec![package("1.10.0", 10), package("1.2.0", 2)],
        vec![EnvironmentMaterialInventoryBaseline::observed(
            "certificate:g038".into(),
            [0x38; 32],
        )],
    )
    .expect("complete baseline seals");

    assert_eq!(
        baseline
            .exact_packages()
            .iter()
            .map(EnvironmentExactPackageBaseline::version)
            .collect::<Vec<_>>(),
        vec!["1.2.0", "1.10.0"]
    );
}

#[test]
fn exact_package_baseline_does_not_sort_raw_id_version_tuples() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/environment_configuration/apply/baseline.rs"
    ));

    assert!(!source.contains("cmp(&(right.package_id(), right.version()))"));
}
