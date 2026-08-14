use crate::{
    DEFAULT_MAX_BLOB_BYTES, DEFAULT_MAX_CALL_DEPTH, DEFAULT_MAX_OPERATIONS,
    DEFAULT_MAX_STRING_BYTES, DEFAULT_MAX_WALL_TIME_MS, MAX_BLOB_BYTES_LIMIT, MAX_CALL_DEPTH_LIMIT,
    MAX_OPERATIONS_LIMIT, MAX_STRING_BYTES_LIMIT, MAX_WALL_TIME_MS_LIMIT, ProtocolResourceLimit,
    ProtocolRuntimeError, ProtocolRuntimeLimits,
};

fn values(limits: ProtocolRuntimeLimits) -> [u64; 5] {
    [
        limits.max_operations(),
        limits.max_call_depth(),
        limits.max_string_bytes(),
        limits.max_blob_bytes(),
        limits.max_wall_time_ms(),
    ]
}

#[test]
fn default_runtime_limits_are_nonzero_and_below_hard_caps() {
    let limits = ProtocolRuntimeLimits::default();
    assert_eq!(
        values(limits),
        [
            DEFAULT_MAX_OPERATIONS,
            DEFAULT_MAX_CALL_DEPTH,
            DEFAULT_MAX_STRING_BYTES,
            DEFAULT_MAX_BLOB_BYTES,
            DEFAULT_MAX_WALL_TIME_MS,
        ]
    );
    for (value, maximum) in values(limits).into_iter().zip([
        MAX_OPERATIONS_LIMIT,
        MAX_CALL_DEPTH_LIMIT,
        MAX_STRING_BYTES_LIMIT,
        MAX_BLOB_BYTES_LIMIT,
        MAX_WALL_TIME_MS_LIMIT,
    ]) {
        assert!(value > 0);
        assert!(value <= maximum);
    }
}

#[test]
fn every_runtime_limit_accepts_one_and_its_exact_hard_cap() {
    assert_eq!(
        values(ProtocolRuntimeLimits::new(1, 1, 1, 1, 1).unwrap()),
        [1; 5]
    );
    assert_eq!(
        values(
            ProtocolRuntimeLimits::new(
                MAX_OPERATIONS_LIMIT,
                MAX_CALL_DEPTH_LIMIT,
                MAX_STRING_BYTES_LIMIT,
                MAX_BLOB_BYTES_LIMIT,
                MAX_WALL_TIME_MS_LIMIT,
            )
            .unwrap()
        ),
        [
            MAX_OPERATIONS_LIMIT,
            MAX_CALL_DEPTH_LIMIT,
            MAX_STRING_BYTES_LIMIT,
            MAX_BLOB_BYTES_LIMIT,
            MAX_WALL_TIME_MS_LIMIT,
        ]
    );
}

#[test]
fn every_runtime_limit_rejects_zero_and_one_above_its_hard_cap() {
    let defaults = ProtocolRuntimeLimits::default();
    let cases = [
        (
            ProtocolResourceLimit::Operations,
            MAX_OPERATIONS_LIMIT,
            [
                0,
                defaults.max_call_depth(),
                defaults.max_string_bytes(),
                defaults.max_blob_bytes(),
                defaults.max_wall_time_ms(),
            ],
        ),
        (
            ProtocolResourceLimit::CallDepth,
            MAX_CALL_DEPTH_LIMIT,
            [
                defaults.max_operations(),
                0,
                defaults.max_string_bytes(),
                defaults.max_blob_bytes(),
                defaults.max_wall_time_ms(),
            ],
        ),
        (
            ProtocolResourceLimit::StringBytes,
            MAX_STRING_BYTES_LIMIT,
            [
                defaults.max_operations(),
                defaults.max_call_depth(),
                0,
                defaults.max_blob_bytes(),
                defaults.max_wall_time_ms(),
            ],
        ),
        (
            ProtocolResourceLimit::BlobBytes,
            MAX_BLOB_BYTES_LIMIT,
            [
                defaults.max_operations(),
                defaults.max_call_depth(),
                defaults.max_string_bytes(),
                0,
                defaults.max_wall_time_ms(),
            ],
        ),
        (
            ProtocolResourceLimit::WallTimeMs,
            MAX_WALL_TIME_MS_LIMIT,
            [
                defaults.max_operations(),
                defaults.max_call_depth(),
                defaults.max_string_bytes(),
                defaults.max_blob_bytes(),
                0,
            ],
        ),
    ];

    for (limit, maximum, mut input) in cases {
        for invalid in [0, maximum + 1] {
            let index = match limit {
                ProtocolResourceLimit::Operations => 0,
                ProtocolResourceLimit::CallDepth => 1,
                ProtocolResourceLimit::StringBytes => 2,
                ProtocolResourceLimit::BlobBytes => 3,
                ProtocolResourceLimit::WallTimeMs => 4,
            };
            input[index] = invalid;
            let error =
                ProtocolRuntimeLimits::new(input[0], input[1], input[2], input[3], input[4])
                    .unwrap_err();
            assert_eq!(
                error,
                ProtocolRuntimeError::InvalidResourceLimit {
                    limit,
                    value: invalid,
                    maximum,
                }
            );
        }
    }
}

#[test]
fn runtime_limit_serde_round_trip_revalidates_and_rejects_unknown_fields() {
    let limits = ProtocolRuntimeLimits::default();
    let value = serde_json::to_value(limits).unwrap();
    assert_eq!(
        serde_json::from_value::<ProtocolRuntimeLimits>(value.clone()).unwrap(),
        limits
    );

    let mut zero = value.clone();
    zero["max_operations"] = serde_json::json!(0);
    assert!(serde_json::from_value::<ProtocolRuntimeLimits>(zero).is_err());

    let mut above_cap = value.clone();
    above_cap["max_blob_bytes"] = serde_json::json!(MAX_BLOB_BYTES_LIMIT + 1);
    assert!(serde_json::from_value::<ProtocolRuntimeLimits>(above_cap).is_err());

    let mut unknown = value;
    unknown["allow_unbounded_execution"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProtocolRuntimeLimits>(unknown).is_err());
}
