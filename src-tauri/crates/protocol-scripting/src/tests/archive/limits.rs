use crate::{
    DEFAULT_MAX_ARCHIVE_BYTES, DEFAULT_MAX_ARCHIVE_ENTRIES, DEFAULT_MAX_COMPRESSION_RATIO,
    DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_PATH_DEPTH, DEFAULT_MAX_TOTAL_BYTES,
    MAX_ARCHIVE_BYTES_LIMIT, MAX_ARCHIVE_ENTRIES_LIMIT, MAX_COMPRESSION_RATIO_LIMIT,
    MAX_FILE_BYTES_LIMIT, MAX_PATH_DEPTH_LIMIT, MAX_TOTAL_BYTES_LIMIT, ProtocolArchiveErrorCode,
    ProtocolArchiveLimits,
};

#[test]
fn default_limits_match_the_documented_host_policy() {
    let limits = ProtocolArchiveLimits::default();
    assert_eq!(limits.max_archive_bytes(), DEFAULT_MAX_ARCHIVE_BYTES);
    assert_eq!(limits.max_entries(), DEFAULT_MAX_ARCHIVE_ENTRIES);
    assert_eq!(limits.max_file_bytes(), DEFAULT_MAX_FILE_BYTES);
    assert_eq!(limits.max_total_bytes(), DEFAULT_MAX_TOTAL_BYTES);
    assert_eq!(
        limits.max_compression_ratio(),
        DEFAULT_MAX_COMPRESSION_RATIO
    );
    assert_eq!(limits.max_path_depth(), DEFAULT_MAX_PATH_DEPTH);
}

#[test]
fn limits_accept_exact_minimum_and_hard_maximum_boundaries() {
    let minimum = ProtocolArchiveLimits::new(1, 1, 1, 1, 1, 1).unwrap();
    assert_eq!(minimum.max_archive_bytes(), 1);
    assert_eq!(minimum.max_total_bytes(), 1);

    let maximum = ProtocolArchiveLimits::new(
        MAX_ARCHIVE_BYTES_LIMIT,
        MAX_ARCHIVE_ENTRIES_LIMIT,
        MAX_FILE_BYTES_LIMIT,
        MAX_TOTAL_BYTES_LIMIT,
        MAX_COMPRESSION_RATIO_LIMIT,
        MAX_PATH_DEPTH_LIMIT,
    )
    .unwrap();
    assert_eq!(maximum.max_entries(), MAX_ARCHIVE_ENTRIES_LIMIT);
    assert_eq!(maximum.max_path_depth(), MAX_PATH_DEPTH_LIMIT);
}

#[test]
fn every_invalid_limit_dimension_fails_closed() {
    let valid = [100_u64, 10, 20, 40, 5, 4];
    let cases = [
        [0, valid[1], valid[2], valid[3], valid[4], valid[5]],
        [
            MAX_ARCHIVE_BYTES_LIMIT + 1,
            valid[1],
            valid[2],
            valid[3],
            valid[4],
            valid[5],
        ],
        [valid[0], 0, valid[2], valid[3], valid[4], valid[5]],
        [
            valid[0],
            MAX_ARCHIVE_ENTRIES_LIMIT as u64 + 1,
            valid[2],
            valid[3],
            valid[4],
            valid[5],
        ],
        [valid[0], valid[1], 0, valid[3], valid[4], valid[5]],
        [
            valid[0],
            valid[1],
            MAX_FILE_BYTES_LIMIT + 1,
            MAX_TOTAL_BYTES_LIMIT,
            valid[4],
            valid[5],
        ],
        [valid[0], valid[1], valid[2], 0, valid[4], valid[5]],
        [valid[0], valid[1], 41, 40, valid[4], valid[5]],
        [
            valid[0],
            valid[1],
            valid[2],
            MAX_TOTAL_BYTES_LIMIT + 1,
            valid[4],
            valid[5],
        ],
        [valid[0], valid[1], valid[2], valid[3], 0, valid[5]],
        [
            valid[0],
            valid[1],
            valid[2],
            valid[3],
            MAX_COMPRESSION_RATIO_LIMIT + 1,
            valid[5],
        ],
        [valid[0], valid[1], valid[2], valid[3], valid[4], 0],
        [
            valid[0],
            valid[1],
            valid[2],
            valid[3],
            valid[4],
            MAX_PATH_DEPTH_LIMIT as u64 + 1,
        ],
    ];
    for values in cases {
        let error = ProtocolArchiveLimits::new(
            values[0],
            usize::try_from(values[1]).unwrap(),
            values[2],
            values[3],
            values[4],
            usize::try_from(values[5]).unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.code(), ProtocolArchiveErrorCode::InvalidLimits);
    }
}

#[test]
fn limit_serde_round_trip_revalidates_and_rejects_unknown_keys() {
    let limits = ProtocolArchiveLimits::new(1000, 12, 300, 900, 20, 6).unwrap();
    let value = serde_json::to_value(&limits).unwrap();
    assert_eq!(
        serde_json::from_value::<ProtocolArchiveLimits>(value.clone()).unwrap(),
        limits
    );

    let mut unknown = value.clone();
    unknown["unbounded"] = true.into();
    assert!(serde_json::from_value::<ProtocolArchiveLimits>(unknown).is_err());

    let mut invalid = value;
    invalid["max_file_bytes"] = 901.into();
    assert!(serde_json::from_value::<ProtocolArchiveLimits>(invalid).is_err());
}
