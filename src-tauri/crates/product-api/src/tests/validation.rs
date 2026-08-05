use crate::{validate_product_profile, validation::valid_https_origin};

use super::support::{
    DUPLICATE_FAULTS, DUPLICATE_PORTS, INVALID_ID, INVALID_URL, TestProfile, UNKNOWN_CAPABILITY,
    UNKNOWN_CHANNEL_FAULTS, VALID_CHANNELS, VALID_FAULTS, storage,
};

#[test]
fn profile_validation_accepts_runtime_channel_id_grammar() {
    validate_product_profile(&TestProfile {
        channels: VALID_CHANNELS,
        storage: storage(),
        faults: VALID_FAULTS,
    })
    .unwrap();
}

#[test]
fn profile_validation_rejects_every_cross_boundary_invariant() {
    let mut empty_storage = storage();
    empty_storage.secret_service = "";
    for profile in [
        TestProfile {
            channels: INVALID_ID,
            storage: storage(),
            faults: &[],
        },
        TestProfile {
            channels: DUPLICATE_PORTS,
            storage: storage(),
            faults: &[],
        },
        TestProfile {
            channels: INVALID_URL,
            storage: storage(),
            faults: &[],
        },
        TestProfile {
            channels: VALID_CHANNELS,
            storage: empty_storage,
            faults: VALID_FAULTS,
        },
        TestProfile {
            channels: VALID_CHANNELS,
            storage: storage(),
            faults: UNKNOWN_CHANNEL_FAULTS,
        },
        TestProfile {
            channels: VALID_CHANNELS,
            storage: storage(),
            faults: DUPLICATE_FAULTS,
        },
        TestProfile {
            channels: VALID_CHANNELS,
            storage: storage(),
            faults: UNKNOWN_CAPABILITY,
        },
    ] {
        assert_eq!(
            validate_product_profile(&profile).unwrap_err().code,
            "PRODUCT_PROFILE_INVALID"
        );
    }
}

#[test]
fn profile_validation_accepts_an_empty_compile_time_channel_catalog() {
    validate_product_profile(&TestProfile {
        channels: &[],
        storage: storage(),
        faults: &[],
    })
    .expect("dynamic Workspace listeners do not require product channels");
}

#[test]
fn profile_validation_rejects_database_paths_outside_the_product_directory() {
    for database_file_name in [
        "../escape.sqlite3",
        "/tmp/escape.sqlite3",
        r"..\escape.sqlite3",
        r"C:\escape.sqlite3",
        ".",
        "..",
    ] {
        let mut invalid_storage = storage();
        invalid_storage.database_file_name = database_file_name;
        let error = validate_product_profile(&TestProfile {
            channels: VALID_CHANNELS,
            storage: invalid_storage,
            faults: VALID_FAULTS,
        })
        .expect_err("database path must remain inside the product data directory");
        assert_eq!(error.code, "PRODUCT_PROFILE_INVALID");
    }
}

#[test]
fn product_channels_accept_only_https_origins() {
    for invalid in [
        "http://alpha.example.test",
        "https://alpha.example.test/base",
        "https://alpha.example.test?mode=test",
        "https://alpha.example.test/#fragment",
        "https://user@alpha.example.test",
        " https://alpha.example.test ",
    ] {
        assert!(!valid_https_origin(invalid), "{invalid:?} must be rejected");
    }
    for valid in [
        "https://alpha.example.test",
        "https://alpha.example.test/",
        "https://alpha.example.test:443",
        "https://[2001:db8::1]:443",
    ] {
        assert!(valid_https_origin(valid), "{valid:?} must be accepted");
    }
}
