use std::{collections::BTreeMap, sync::Arc};

use crate::{
    PackageFilePath, ProtocolPackageCompiler, ProtocolPackageFiles, ProtocolRuntimeLimits,
    host::context::ProtocolDirection,
};

use super::{
    FrameBuffer, FramingDecision, ProtocolFramingError, ProtocolFramingErrorCode,
    ProtocolFramingLimit, ProtocolFramingLimits, ProtocolReader, ReaderSegment,
    framer::SingleDirectionFramer, script::RhaiFrameDecider,
};

const DOCUMENT_SCHEMA: &str = r#"id = "framing-test"
version = 1
title = "Framing Test"

[[fields]]
name = "kind"
label = "Kind"
type = "int"
"#;

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

    // 生产构造器始终保证 segments 总长度等于 available；此畸形对象只验证未来若破坏该不变量，
    // Reader 仍然返回受控越界错误而不是 panic。
    let malformed = ProtocolReader::from_segments(Vec::new(), 1);
    assert_eq!(
        malformed.peek_u8(0).unwrap_err(),
        ProtocolFramingError::ReaderOutOfBounds
    );
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
        ProtocolReader::empty().find(b"A", 0).unwrap_err(),
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
            ProtocolFramingError::FrameEntryFailed { package },
            ProtocolFramingErrorCode::FrameEntryFailed,
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

#[test]
fn fixed_length_supports_byte_chunks_whole_frames_and_sticky_packets() {
    let mut framer = closure_framer(4, 8, |reader: ProtocolReader| {
        if reader.available() < 4 {
            Ok(FramingDecision::NeedMore(4))
        } else {
            Ok(FramingDecision::Complete(4))
        }
    });

    assert!(framer.push(vec![1]).unwrap().is_empty());
    assert!(framer.push(vec![2]).unwrap().is_empty());
    assert!(framer.push(vec![3]).unwrap().is_empty());
    assert_eq!(framer.push(vec![4]).unwrap(), vec![vec![1, 2, 3, 4]]);
    assert_eq!(
        framer.push(vec![5, 6, 7, 8]).unwrap(),
        vec![vec![5, 6, 7, 8]]
    );
    assert_eq!(
        framer.push(vec![9, 10, 11, 12, 13, 14, 15, 16]).unwrap(),
        vec![vec![9, 10, 11, 12], vec![13, 14, 15, 16]]
    );
}

#[test]
fn big_endian_length_prefix_handles_split_header_and_payload() {
    let mut framer = closure_framer(32, 32, |reader: ProtocolReader| {
        if reader.available() < 2 {
            return Ok(FramingDecision::NeedMore(2));
        }
        let total = 2 + usize::from(reader.peek_u16_be(0)?);
        if reader.available() < total {
            Ok(FramingDecision::NeedMore(total))
        } else {
            Ok(FramingDecision::Complete(total))
        }
    });

    assert!(framer.push(vec![0]).unwrap().is_empty());
    assert!(framer.push(vec![3, b'a']).unwrap().is_empty());
    assert_eq!(
        framer.push(vec![b'b', b'c']).unwrap(),
        vec![vec![0, 3, b'a', b'b', b'c']]
    );
}

#[test]
fn little_endian_length_prefix_can_cut_multiple_frames_from_one_chunk() {
    let mut framer = closure_framer(32, 32, |reader: ProtocolReader| {
        if reader.available() < 2 {
            return Ok(FramingDecision::NeedMore(2));
        }
        let total = 2 + usize::from(reader.peek_u16_le(0)?);
        if reader.available() < total {
            Ok(FramingDecision::NeedMore(total))
        } else {
            Ok(FramingDecision::Complete(total))
        }
    });

    assert_eq!(
        framer.push(vec![1, 0, b'A', 2, 0, b'B', b'C']).unwrap(),
        vec![vec![1, 0, b'A'], vec![2, 0, b'B', b'C']]
    );
}

#[test]
fn delimiter_magic_and_tlv_strategies_are_protocol_agnostic() {
    let mut delimiter = closure_framer(32, 32, |reader: ProtocolReader| {
        match reader.find(b"\r\n", 0)? {
            Some(offset) => Ok(FramingDecision::Complete(offset + 2)),
            None => Ok(FramingDecision::NeedMore(reader.available() + 1)),
        }
    });
    assert_eq!(
        delimiter.push(b"ONE\r\nTWO\r\n".to_vec()).unwrap(),
        vec![b"ONE\r\n".to_vec(), b"TWO\r\n".to_vec()]
    );

    let mut magic = closure_framer(32, 32, |reader: ProtocolReader| {
        if reader.available() < 4 {
            return Ok(FramingDecision::NeedMore(4));
        }
        if reader.peek(0, 2)? != b"MG" {
            return Ok(FramingDecision::Reject("magic mismatch".to_owned()));
        }
        let total = 4 + usize::from(reader.peek_u16_be(2)?);
        if reader.available() < total {
            Ok(FramingDecision::NeedMore(total))
        } else {
            Ok(FramingDecision::Complete(total))
        }
    });
    assert_eq!(
        magic.push(vec![b'M', b'G', 0, 2, 7, 8]).unwrap(),
        vec![vec![b'M', b'G', 0, 2, 7, 8]]
    );

    // TLV 示例：第 0 字节是 tag，第 1 字节是 value 长度。
    let mut tlv = closure_framer(16, 16, |reader: ProtocolReader| {
        if reader.available() < 2 {
            return Ok(FramingDecision::NeedMore(2));
        }
        let total = 2 + usize::from(reader.peek_u8(1)?);
        if reader.available() < total {
            Ok(FramingDecision::NeedMore(total))
        } else {
            Ok(FramingDecision::Complete(total))
        }
    });
    assert_eq!(
        tlv.push(vec![0x10, 2, 0xaa, 0xbb]).unwrap(),
        vec![vec![0x10, 2, 0xaa, 0xbb]]
    );
}

#[test]
fn invalid_decisions_reject_every_state_machine_boundary() {
    assert_state_error(
        FramingDecision::NeedMore(1),
        &ProtocolFramingError::NeedMoreWithoutProgress,
    );
    assert_state_error(
        FramingDecision::Complete(0),
        &ProtocolFramingError::CompleteEmpty,
    );
    assert_state_error(
        FramingDecision::Complete(2),
        &ProtocolFramingError::CompleteOutOfBounds,
    );
    assert_state_error(
        FramingDecision::NeedMore(9),
        &ProtocolFramingError::FrameTooLarge {
            frame_bytes: 9,
            maximum: 8,
        },
    );
    let mut oversized_complete = closure_framer(8, 16, |_| Ok(FramingDecision::Complete(9)));
    assert_eq!(
        oversized_complete.push(vec![0; 9]).unwrap_err(),
        ProtocolFramingError::FrameTooLarge {
            frame_bytes: 9,
            maximum: 8,
        }
    );

    let mut rejected = closure_framer(8, 8, |_| {
        Ok(FramingDecision::Reject("bad magic".to_owned()))
    });
    assert_eq!(
        rejected.push(vec![1]).unwrap_err(),
        ProtocolFramingError::Rejected {
            reason: "bad magic".to_owned(),
        }
    );
    assert_eq!(rejected.buffered_bytes(), 0);

    assert_state_error(
        FramingDecision::Reject(String::new()),
        &ProtocolFramingError::InvalidRejectReason,
    );
}

#[test]
fn buffer_stays_bounded_and_large_sticky_chunks_drain_incrementally() {
    let mut blocked = closure_framer(8, 8, |reader: ProtocolReader| {
        Ok(FramingDecision::NeedMore(reader.available() + 1))
    });
    assert_eq!(
        blocked.push(vec![0; 9]).unwrap_err(),
        ProtocolFramingError::FrameTooLarge {
            frame_bytes: 9,
            maximum: 8,
        }
    );
    assert_eq!(blocked.buffered_bytes(), 0);

    // 同样是 16 字节 chunk，但每 4 字节可释放一次，所以 8 字节 FIFO 足够承载。
    let mut draining = closure_framer(4, 8, |reader: ProtocolReader| {
        if reader.available() < 4 {
            Ok(FramingDecision::NeedMore(4))
        } else {
            Ok(FramingDecision::Complete(4))
        }
    });
    assert_eq!(draining.push(vec![7; 16]).unwrap().len(), 4);
    assert_eq!(draining.buffered_bytes(), 0);

    // 正常状态机不可能在一次成功 push 后留下“恰好满且仍需更多”的 FIFO；强制构造该内部状态，
    // 锁定防御分支仍会清空内存并返回稳定资源错误。
    let mut invariant_guard = closure_framer(8, 8, |_| Ok(FramingDecision::NeedMore(8)));
    invariant_guard.force_full_fifo_for_invariant_test();
    assert_eq!(
        invariant_guard.push(vec![1]).unwrap_err(),
        ProtocolFramingError::FifoLimitExceeded { maximum: 8 }
    );
    assert_eq!(invariant_guard.buffered_bytes(), 0);
}

#[test]
fn eof_cancel_and_drop_release_directional_buffer_memory() {
    let mut eof = closure_framer(8, 8, |_| Ok(FramingDecision::NeedMore(4)));
    eof.push(vec![1, 2]).unwrap();
    assert_eq!(
        eof.finish().unwrap_err(),
        ProtocolFramingError::TruncatedFrame { buffered_bytes: 2 }
    );
    assert_eq!(eof.buffered_bytes(), 0);
    assert_eq!(eof.buffer_chunk_capacity(), 0);
    assert!(eof.finish().is_ok());

    let mut cancelled = closure_framer(8, 8, |_| Ok(FramingDecision::NeedMore(4)));
    cancelled.push(vec![1, 2]).unwrap();
    cancelled.cancel();
    assert_eq!(cancelled.buffered_bytes(), 0);
    assert_eq!(cancelled.buffer_chunk_capacity(), 0);

    let bytes: Arc<[u8]> = vec![1, 2, 3].into();
    let weak = Arc::downgrade(&bytes);
    let mut buffer = FrameBuffer::default();
    buffer.append(Arc::clone(&bytes), 0..0);
    assert!(buffer.is_empty());
    buffer.append(Arc::clone(&bytes), 0..3);
    drop(bytes);
    assert!(weak.upgrade().is_some());
    drop(buffer);
    assert!(weak.upgrade().is_none());
}

#[test]
fn two_direction_instances_never_share_fifo_state() {
    let mut upstream = closure_framer(2, 4, |reader: ProtocolReader| {
        if reader.available() < 2 {
            Ok(FramingDecision::NeedMore(2))
        } else {
            Ok(FramingDecision::Complete(2))
        }
    });
    let mut downstream = closure_framer(3, 6, |reader: ProtocolReader| {
        if reader.available() < 3 {
            Ok(FramingDecision::NeedMore(3))
        } else {
            Ok(FramingDecision::Complete(3))
        }
    });

    assert!(upstream.push(vec![1]).unwrap().is_empty());
    assert!(downstream.push(vec![9, 8]).unwrap().is_empty());
    assert_eq!(upstream.push(vec![2]).unwrap(), vec![vec![1, 2]]);
    assert_eq!(downstream.push(vec![7]).unwrap(), vec![vec![9, 8, 7]]);
}

#[test]
fn rhai_frame_entry_receives_reader_and_read_only_direction_context() {
    let upstream_script = r#"
fn frame(reader, context) {
    if context.direction() != "upstream" || context.stage() != "receive" {
        return framing::reject("wrong context");
    }
    if reader.available() < 2 { return framing::need_more(2); }
    let total = 2 + reader.peek_u16_be(0);
    if reader.available() < total { framing::need_more(total) }
    else { framing::complete(total) }
}
fn decode(origin, context) { () }
"#;
    let package = compile_package(upstream_script, valid_fixed_script());
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-1",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer =
        SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(64, 64).unwrap());

    assert!(framer.push(vec![0]).unwrap().is_empty());
    assert!(framer.push(vec![2, b'A']).unwrap().is_empty());
    assert_eq!(
        framer.push(vec![b'B']).unwrap(),
        vec![vec![0, 2, b'A', b'B']]
    );
}

#[test]
fn rhai_frame_wrong_return_type_is_fail_closed_and_clears_fifo() {
    let package = compile_package(
        "fn frame(reader, context) { () }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-2",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer = SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(8, 8).unwrap());

    assert_eq!(
        framer.push(vec![1]).unwrap_err(),
        ProtocolFramingError::FrameEntryFailed {
            package: package.package().clone(),
        }
    );
    assert_eq!(framer.buffered_bytes(), 0);
}

#[test]
fn rhai_reader_find_and_static_imports_work_without_a_runtime_file_resolver() {
    let upstream = r#"
import "libraries/framing" as framing_helpers;

fn frame(reader, context) {
    framing_helpers::delimiter_frame(reader)
}
fn decode(origin, context) { () }
"#;
    let library = r#"
fn delimiter_frame(reader) {
    let delimiter = "\r\n".to_blob();
    let offset = reader.find(delimiter, 0);
    if offset < 0 { framing::need_more(reader.available() + 1) }
    else { framing::complete(offset + delimiter.len()) }
}
"#;
    let package = compile_package_with_files(
        upstream,
        valid_fixed_script(),
        &[("libraries/framing.rhai", library.as_bytes())],
    );
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-import",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer =
        SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(64, 64).unwrap());

    assert!(framer.push(b"ONE\r".to_vec()).unwrap().is_empty());
    assert_eq!(
        framer.push(b"\nTWO\r\n".to_vec()).unwrap(),
        vec![b"ONE\r\n".to_vec(), b"TWO\r\n".to_vec()]
    );
}

#[test]
fn rhai_host_rejects_negative_lengths_empty_reasons_and_reader_misuse() {
    let invalid_bodies = [
        "framing::need_more(-1)",
        "framing::complete(-1)",
        "framing::reject(\"\")",
        "reader.peek(-1, 1); framing::complete(1)",
        "reader.peek(1, 99); framing::complete(1)",
        "reader.find(blob(), 0); framing::complete(1)",
    ];

    for (index, body) in invalid_bodies.into_iter().enumerate() {
        let script = format!(
            "fn frame(reader, context) {{ {body} }}\nfn decode(origin, context) {{ () }}\n"
        );
        let package = compile_package(&script, valid_fixed_script());
        let decider = RhaiFrameDecider::for_package(
            &package,
            ProtocolDirection::Upstream,
            format!("connection-invalid-{index}"),
            "listener-1",
            ProtocolRuntimeLimits::default(),
        );
        let mut framer =
            SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(8, 8).unwrap());
        assert_eq!(
            framer.push(vec![1]).unwrap_err().code(),
            ProtocolFramingErrorCode::FrameEntryFailed,
            "invalid Rhai body unexpectedly succeeded: {body}"
        );
        assert_eq!(framer.buffered_bytes(), 0);
    }
}

#[test]
fn rhai_reader_registers_every_integer_and_blob_method_for_downstream() {
    let downstream = r#"
fn frame(reader, context) {
    if context.direction() != "downstream" { return framing::reject("wrong direction"); }
    if reader.available() < 8 { return framing::need_more(8); }
    if reader.peek(1, 2).len() != 2 { return framing::reject("peek"); }
    if reader.peek_u8(0) != 1 { return framing::reject("u8"); }
    if reader.peek_u16_be(0) != 0x0102 { return framing::reject("u16be"); }
    if reader.peek_u16_le(0) != 0x0201 { return framing::reject("u16le"); }
    if reader.peek_u32_be(0) != 0x01020304 { return framing::reject("u32be"); }
    if reader.peek_u32_le(0) != 0x04030201 { return framing::reject("u32le"); }
    if reader.find(reader.peek(2, 2), 0) != 2 { return framing::reject("find"); }
    framing::complete(8)
}
fn decode(origin, context) { () }
"#;
    let package = compile_package(valid_fixed_script(), downstream);
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Downstream,
        "connection-reader",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer =
        SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(16, 16).unwrap());

    assert_eq!(
        framer.push(vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap(),
        vec![vec![1, 2, 3, 4, 5, 6, 7, 8]]
    );
}

#[test]
fn rhai_reject_constructor_accepts_a_bounded_reason() {
    let package = compile_package(
        "fn frame(reader, context) { framing::reject(\"not mine\") }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-reject",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer = SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(8, 8).unwrap());
    assert_eq!(
        framer.push(vec![1]).unwrap_err(),
        ProtocolFramingError::Rejected {
            reason: "not mine".to_owned(),
        }
    );
}

#[test]
fn official_iso8583_template_frame_executes_with_globals_and_embedded_imports() {
    let package = compile_official_iso8583_package();
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-iso8583",
        "listener-iso8583",
        ProtocolRuntimeLimits::default(),
    );
    let mut framer = SingleDirectionFramer::new(
        decider,
        ProtocolFramingLimits::new(65_535, 131_070).unwrap(),
    );

    assert!(framer.push(vec![0]).unwrap().is_empty());
    assert!(framer.push(vec![4, b'0', b'2']).unwrap().is_empty());
    assert_eq!(
        framer.push(vec![b'0', b'0']).unwrap(),
        vec![vec![0, 4, b'0', b'2', b'0', b'0']]
    );
}

#[test]
fn rhai_operation_limit_stops_a_non_terminating_frame_entry() {
    let package = compile_package(
        "fn frame(reader, context) { while true {} }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let runtime_limits = ProtocolRuntimeLimits::new(100, 32, 1024, 1024, 250).unwrap();
    let decider = RhaiFrameDecider::for_package(
        &package,
        ProtocolDirection::Upstream,
        "connection-loop",
        "listener-1",
        runtime_limits,
    );
    let mut framer = SingleDirectionFramer::new(decider, ProtocolFramingLimits::new(8, 8).unwrap());

    assert_eq!(
        framer.push(vec![1]).unwrap_err().code(),
        ProtocolFramingErrorCode::FrameEntryFailed
    );
    assert_eq!(framer.buffered_bytes(), 0);
}

fn reader(parts: &[&[u8]]) -> ProtocolReader {
    let mut available = 0;
    let segments = parts
        .iter()
        .map(|part| {
            available += part.len();
            let bytes: Arc<[u8]> = part.to_vec().into();
            ReaderSegment::new(bytes, 0..part.len())
        })
        .collect();
    ProtocolReader::from_segments(segments, available)
}

fn closure_framer<F>(max_frame: u64, max_fifo: u64, decider: F) -> SingleDirectionFramer<F>
where
    F: FnMut(ProtocolReader) -> Result<FramingDecision, ProtocolFramingError>,
{
    SingleDirectionFramer::new(
        decider,
        ProtocolFramingLimits::new(max_frame, max_fifo).unwrap(),
    )
}

fn assert_state_error(decision: FramingDecision, expected: &ProtocolFramingError) {
    let mut decision = Some(decision);
    let mut framer = closure_framer(8, 8, move |_| Ok(decision.take().unwrap()));
    assert_eq!(&framer.push(vec![1]).unwrap_err(), expected);
    assert_eq!(framer.buffered_bytes(), 0);
}

fn valid_fixed_script() -> &'static str {
    "fn frame(reader, context) { if reader.available() < 1 { framing::need_more(1) } else { framing::complete(1) } }\nfn decode(origin, context) { () }\n"
}

fn compile_package(
    upstream_script: &str,
    downstream_script: &str,
) -> crate::CompiledProtocolPackage {
    compile_package_with_files(upstream_script, downstream_script, &[])
}

fn compile_package_with_files(
    upstream_script: &str,
    downstream_script: &str,
    extra_files: &[(&str, &[u8])],
) -> crate::CompiledProtocolPackage {
    let manifest = r#"api = 1

[package]
id = "framing-test"
name = "Framing Test"
version = "1.0.0"

[document]
schema = "document.toml"

[hooks.upstream.receive]
script = "upstream.rhai"
frame = "frame"
decode = "decode"

[hooks.downstream.receive]
script = "downstream.rhai"
frame = "frame"
decode = "decode"
"#;
    let mut files = BTreeMap::from([
        (path("manifest.toml"), manifest.as_bytes().to_vec()),
        (path("document.toml"), DOCUMENT_SCHEMA.as_bytes().to_vec()),
        (path("upstream.rhai"), upstream_script.as_bytes().to_vec()),
        (
            path("downstream.rhai"),
            downstream_script.as_bytes().to_vec(),
        ),
    ]);
    for (name, bytes) in extra_files {
        files.insert(path(name), bytes.to_vec());
    }
    let total_bytes = files.values().map(Vec::len).sum::<usize>();
    let files = ProtocolPackageFiles::new(files, u64::try_from(total_bytes).unwrap());
    ProtocolPackageCompiler::default().compile(&files).unwrap()
}

fn compile_official_iso8583_package() -> crate::CompiledProtocolPackage {
    let manifest =
        include_str!("../../../../../templates/socket-protocol/iso8583-standard/manifest.toml");
    let schema =
        include_bytes!("../../../../../templates/socket-protocol/iso8583-standard/document.toml");
    let protocol =
        include_bytes!("../../../../../templates/socket-protocol/iso8583-standard/protocol.rhai");
    let display =
        include_bytes!("../../../../../templates/socket-protocol/iso8583-standard/display.rhai");
    let library = include_bytes!(
        "../../../../../templates/socket-protocol/iso8583-standard/libraries/iso8583.rhai"
    );
    let files = BTreeMap::from([
        (path("manifest.toml"), manifest.as_bytes().to_vec()),
        (path("document.toml"), schema.to_vec()),
        (path("protocol.rhai"), protocol.to_vec()),
        (path("display.rhai"), display.to_vec()),
        (path("libraries/iso8583.rhai"), library.to_vec()),
    ]);
    let total_bytes = files.values().map(Vec::len).sum::<usize>();
    let files = ProtocolPackageFiles::new(files, u64::try_from(total_bytes).unwrap());
    ProtocolPackageCompiler::default().compile(&files).unwrap()
}

fn path(value: &str) -> PackageFilePath {
    PackageFilePath::new(value).unwrap()
}
