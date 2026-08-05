use std::collections::BTreeSet;

use super::*;
use crate::{DestinationTarget, NetworkProfile, ProxyRoute, WeakNetworkProfile};

fn app(package_name: &str, uid: u32) -> InstalledApplication {
    InstalledApplication {
        package_name: package_name.to_owned(),
        uid,
    }
}

fn profile(targets: &[InstalledApplication]) -> NetworkProfile {
    NetworkProfile {
        id: "test".to_owned(),
        name: "测试".to_owned(),
        target_applications: targets
            .iter()
            .map(|target| TargetApplication {
                package_name: target.package_name.clone(),
                uid: target.uid,
            })
            .collect(),
        destination_targets: Vec::new(),
        proxy_routes: Vec::new(),
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        weak_network: WeakNetworkProfile::default(),
    }
}

#[test]
fn companion_cannot_be_selected() {
    let installed = vec![app(COMPANION_PACKAGE_NAME, 10001)];
    assert_eq!(
        profile(&installed).validate_for_start(&installed),
        Err(ProfileValidationError::CompanionSelected)
    );
}

#[test]
fn partial_shared_uid_group_is_rejected() {
    let first = app("com.example.first", 10001);
    let second = app("com.example.second", 10001);
    let installed = vec![first.clone(), second];
    assert_eq!(
        profile(&[first]).validate_for_start(&installed),
        Err(ProfileValidationError::PartialSharedUidSelection {
            uid: 10001,
            missing_packages: vec!["com.example.second".to_owned()],
        })
    );
}

#[test]
fn complete_shared_uid_group_needs_explicit_confirmation() {
    let installed = vec![
        app("com.example.first", 10001),
        app("com.example.second", 10001),
    ];
    let mut candidate = profile(&installed);
    assert!(matches!(
        candidate.validate_for_start(&installed),
        Err(ProfileValidationError::SharedUidConfirmationRequired { .. })
    ));
    candidate.confirmed_shared_uids.insert(10001);
    assert!(candidate.validate_for_start(&installed).is_ok());
}

#[test]
fn uninstalled_target_is_rejected_before_vpn_start() {
    let saved = app("com.example.target", 10001);
    assert_eq!(
        profile(&[saved]).validate_for_start(&[]),
        Err(ProfileValidationError::PackageNotInstalled(
            "com.example.target".to_owned()
        ))
    );
}

#[test]
fn changed_uid_is_rejected_before_vpn_start() {
    let saved = app("com.example.target", 10001);
    let installed = vec![app("com.example.target", 10002)];
    assert_eq!(
        profile(&[saved]).validate_for_start(&installed),
        Err(ProfileValidationError::UidChanged {
            package_name: "com.example.target".to_owned(),
            saved_uid: 10001,
            actual_uid: 10002,
        })
    );
}

#[test]
fn more_than_sixty_four_targets_are_rejected() {
    let installed = (0..=MAX_TARGET_APPLICATIONS)
        .map(|index| {
            app(
                &format!("com.example.app{index}"),
                10_000 + u32::try_from(index).expect("测试下标不会超过 u32"),
            )
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        profile(&installed).validate_for_start(&installed),
        Err(ProfileValidationError::TooManyTargetApplications {
            maximum: MAX_TARGET_APPLICATIONS,
            actual: 65,
        })
    ));
}

#[test]
fn destination_targets_reject_invalid_cidr_ports_duplicates_and_limits() {
    let installed = vec![app("com.example.target", 10001)];
    let mut candidate = profile(&installed);
    candidate.destination_targets = vec![DestinationTarget {
        cidr: "10.0.0.0/33".to_owned(),
        ports: Vec::new(),
    }];
    assert!(matches!(
        candidate.validate_for_start(&installed),
        Err(ProfileValidationError::InvalidDestinationCidr(_))
    ));

    candidate.destination_targets = vec![DestinationTarget {
        cidr: "10.0.0.1".to_owned(),
        ports: vec![443, 443],
    }];
    assert!(matches!(
        candidate.validate_for_start(&installed),
        Err(ProfileValidationError::InvalidDestinationPorts { .. })
    ));

    let duplicate = DestinationTarget {
        cidr: "2001:db8::/32".to_owned(),
        ports: vec![443],
    };
    candidate.destination_targets = vec![duplicate.clone(), duplicate];
    assert!(matches!(
        candidate.validate_for_start(&installed),
        Err(ProfileValidationError::DuplicateDestinationTarget(_))
    ));

    candidate.destination_targets = (0..129)
        .map(|index| DestinationTarget {
            cidr: format!("10.0.{}.{}", index / 256, index % 256),
            ports: Vec::new(),
        })
        .collect();
    assert!(matches!(
        candidate.validate_for_start(&installed),
        Err(ProfileValidationError::TooManyDestinationTargets {
            maximum: 128,
            actual: 129,
        })
    ));
}

#[test]
fn proxy_routes_reject_equivalent_destination_spellings() {
    let installed = vec![app("com.example.target", 10001)];
    for destinations in [
        ["2001:0db8::1", "2001:db8::1"],
        ["2001:db8:0:1::1/64", "2001:db8:0:1::abcd/64"],
        ["127.0.0.1", "127.0.0.1."],
        ["Example.COM.", "example.com"],
    ] {
        let mut candidate = profile(&installed);
        candidate.proxy_routes = destinations
            .into_iter()
            .enumerate()
            .map(|(index, destination)| ProxyRoute {
                destination: destination.to_owned(),
                ports: vec![8_443],
                listener_id: format!("listener-{index}"),
            })
            .collect();

        assert!(matches!(
            candidate.validate_for_start(&installed),
            Err(ProfileValidationError::DuplicateProxyRoute(_))
        ));
    }
}
