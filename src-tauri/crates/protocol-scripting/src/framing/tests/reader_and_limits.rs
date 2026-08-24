#[test]
fn reader_reads_every_supported_width_and_endianness_across_chunks() {
    let reader = reader(&[&[0x01, 0x02], &[0x03], &[0x04, 0x05]]);

    assert_eq!(reader.available(), 5);
    assert_eq!(reader.peek(1, 3).unwrap(), vec![0x02, 0x03, 0x04]);
    assert_eq!(reader.peek(5, 0).unwrap(), Vec::<u8>::new());
    assert_eq!(reader.peek_u8(2).unwrap(), 0x03);
    assert_eq!(reader.peek_u16_be(1).unwrap(), 0x0203);
    assert_eq!(reader.peek_u16_le(1).unwrap(), 0x0302);
    assert_eq!(reader.peek_u32_be(0).unwrap(), 0x0102_0304);
    assert_eq!(reader.peek_u32_le(0).unwrap(), 0x0403_0201);
}
#[test]
fn reader_rejects_every_invalid_offset_and_length_boundary() {
    let reader = reader(&[&[1, 2, 3]]);
    for result in [
        reader.peek(4, 0).map(|_| ()),
        reader.peek(2, 2).map(|_| ()),
        reader.peek(usize::MAX, 1).map(|_| ()),
        reader.peek_u8(3).map(|_| ()),
        reader.peek_u16_be(2).map(|_| ()),
        reader.peek_u16_le(2).map(|_| ()),
        reader.peek_u32_be(0).map(|_| ()),
        reader.peek_u32_le(0).map(|_| ()),
    ] {
        assert_eq!(result.unwrap_err(), ProtocolFramingError::ReaderOutOfBounds);
    }

}

#[test]
fn reader_find_handles_cross_chunk_missing_empty_and_start_boundaries() {
    let reader = reader(&[b"AB", b"CD", b"EF"]);

    assert_eq!(reader.find(b"BCD", 0).unwrap(), Some(1));
    assert_eq!(reader.find(b"CD", 2).unwrap(), Some(2));
    assert_eq!(reader.find(b"ZZ", 0).unwrap(), None);
    assert_eq!(
        reader.find(b"", 0).unwrap_err(),
        ProtocolFramingError::EmptyFindPattern
    );
    assert_eq!(
        reader.find(b"A", reader.available()).unwrap_err(),
        ProtocolFramingError::InvalidFindStart
    );
    assert_eq!(
        ProtocolReader::new(Arc::from([]))
            .find(b"A", 0)
            .unwrap_err(),
        ProtocolFramingError::InvalidFindStart
    );
    assert_eq!(reader.find(b"ABCDEFG", 0).unwrap(), None);
}

#[test]
fn framing_limits_validate_each_hard_boundary_and_round_trip() {
    assert_eq!(
        ProtocolFramingLimits::new(0, 1).unwrap_err(),
        ProtocolFramingError::InvalidLimit {
            limit: ProtocolFramingLimit::FrameBytes,
            value: 0,
            maximum: super::MAX_FRAME_BYTES_LIMIT,
        }
    );
    assert_eq!(
        ProtocolFramingLimits::new(1, 0).unwrap_err(),
        ProtocolFramingError::InvalidLimit {
            limit: ProtocolFramingLimit::FifoBytes,
            value: 0,
            maximum: super::MAX_FRAME_FIFO_BYTES_LIMIT,
        }
    );
    assert_eq!(
        ProtocolFramingLimits::new(super::MAX_FRAME_BYTES_LIMIT + 1, 32 * 1024 * 1024)
            .unwrap_err()
            .code(),
        ProtocolFramingErrorCode::InvalidLimit
    );
    assert_eq!(
        ProtocolFramingLimits::new(1, super::MAX_FRAME_FIFO_BYTES_LIMIT + 1)
            .unwrap_err()
            .code(),
        ProtocolFramingErrorCode::InvalidLimit
    );
    assert!(
        ProtocolFramingLimits::new(
            super::MAX_FRAME_BYTES_LIMIT,
            super::MAX_FRAME_FIFO_BYTES_LIMIT,
        )
        .is_ok()
    );
    assert_eq!(
        ProtocolFramingLimits::new(8, 7).unwrap_err(),
        ProtocolFramingError::FifoSmallerThanFrame {
            frame_bytes: 8,
            fifo_bytes: 7,
        }
    );

    let limits = ProtocolFramingLimits::new(8, 16).unwrap();
    assert_eq!(limits.max_frame_bytes(), 8);
    assert_eq!(limits.max_fifo_bytes(), 16);
    let json = serde_json::to_string(&limits).unwrap();
    assert_eq!(json, r#"{"max_frame_bytes":8,"max_fifo_bytes":16}"#);
    assert_eq!(
        serde_json::from_str::<ProtocolFramingLimits>(&json).unwrap(),
        limits
    );
    assert!(
        serde_json::from_str::<ProtocolFramingLimits>(
            r#"{"max_frame_bytes":8,"max_fifo_bytes":16,"unknown":1}"#
        )
        .is_err()
    );

    let defaults = ProtocolFramingLimits::default();
    assert_eq!(defaults.max_frame_bytes(), super::DEFAULT_MAX_FRAME_BYTES);
    assert_eq!(
        defaults.max_fifo_bytes(),
        super::DEFAULT_MAX_FRAME_FIFO_BYTES
    );
}

#[test]
fn framing_errors_cover_every_stable_code_display_and_wire_shape() {
    let package = crate::test_support::CompiledProtocolPackageTestBuilder::new()
        .build()
        .package()
        .clone();
    let cases = [
        (
            ProtocolFramingError::InvalidLimit {
                limit: ProtocolFramingLimit::FrameBytes,
                value: 0,
                maximum: 8,
            },
            ProtocolFramingErrorCode::InvalidLimit,
        ),
        (
            ProtocolFramingError::FifoSmallerThanFrame {
                frame_bytes: 8,
                fifo_bytes: 4,
            },
            ProtocolFramingErrorCode::FifoSmallerThanFrame,
        ),
        (
            ProtocolFramingError::ReaderOutOfBounds,
            ProtocolFramingErrorCode::ReaderOutOfBounds,
        ),
        (
            ProtocolFramingError::EmptyFindPattern,
            ProtocolFramingErrorCode::EmptyFindPattern,
        ),
        (
            ProtocolFramingError::InvalidFindStart,
            ProtocolFramingErrorCode::InvalidFindStart,
        ),
        (
            ProtocolFramingError::InvalidDecisionLength,
            ProtocolFramingErrorCode::InvalidDecisionLength,
        ),
        (
            ProtocolFramingError::InvalidRejectReason,
            ProtocolFramingErrorCode::InvalidRejectReason,
        ),
        (
            ProtocolFramingError::NeedMoreWithoutProgress,
            ProtocolFramingErrorCode::NeedMoreWithoutProgress,
        ),
        (
            ProtocolFramingError::CompleteEmpty,
            ProtocolFramingErrorCode::CompleteEmpty,
        ),
        (
            ProtocolFramingError::CompleteOutOfBounds,
            ProtocolFramingErrorCode::CompleteOutOfBounds,
        ),
        (
            ProtocolFramingError::FrameTooLarge {
                frame_bytes: 9,
                maximum: 8,
            },
            ProtocolFramingErrorCode::FrameTooLarge,
        ),
        (
            ProtocolFramingError::FifoLimitExceeded { maximum: 8 },
            ProtocolFramingErrorCode::FifoLimitExceeded,
        ),
        (
            ProtocolFramingError::Rejected {
                reason: "bad magic".to_owned(),
            },
            ProtocolFramingErrorCode::Rejected,
        ),
        (
            ProtocolFramingError::FrameEntryFailed {
                package: package.clone(),
            },
            ProtocolFramingErrorCode::FrameEntryFailed,
        ),
        (
            ProtocolFramingError::FrameExecutionCancelled { package },
            ProtocolFramingErrorCode::FrameExecutionCancelled,
        ),
        (
            ProtocolFramingError::TruncatedFrame { buffered_bytes: 3 },
            ProtocolFramingErrorCode::TruncatedFrame,
        ),
    ];

    assert_eq!(ProtocolFramingLimit::FrameBytes.to_string(), "frame_bytes");
    assert_eq!(ProtocolFramingLimit::FifoBytes.to_string(), "fifo_bytes");
    for (error, code) in cases {
        assert_eq!(error.code(), code);
        assert!(!error.to_string().is_empty());
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(
            serde_json::from_str::<ProtocolFramingError>(&json).unwrap(),
            error
        );
    }
}
