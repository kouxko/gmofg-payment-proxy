use crate::{InterceptProxyProfile, ProductProfile, validate_product_profile};

#[test]
fn intercept_profile_is_clean_and_declares_no_product_channels() {
    let profile = InterceptProxyProfile;
    validate_product_profile(&profile).expect("generic profile must be valid");
    assert_eq!(profile.name(), "Intercept Proxy");
    assert_eq!(
        profile.storage().database_file_name,
        "intercept-proxy.sqlite3"
    );
    assert!(profile.channels().is_empty());
    assert!(
        profile
            .certificates()
            .fixed_installation_root_ca_pem()
            .is_some()
    );
    assert!(
        profile
            .certificates()
            .fixed_installation_root_key_pem()
            .is_some()
    );
    assert!(profile.certificates().bundled_upstream_ca_pem().is_none());
}
