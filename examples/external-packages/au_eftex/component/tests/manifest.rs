use intercept_proxy_package_contract::{PackageKind, PackageManifest};

#[test]
fn manifest_declares_the_socket_component_and_both_direction_schemas() {
    let manifest = serde_json::from_str::<PackageManifest>(include_str!("../manifest.json"))
        .expect("valid AU EFTEX Component manifest");

    assert_eq!(manifest.kind(), PackageKind::Socket);
    assert_eq!(manifest.package().identity().id.as_str(), "au-eftex");
    assert!(manifest.document().upstream().schema().is_some());
    assert!(manifest.document().downstream().schema().is_some());
}
