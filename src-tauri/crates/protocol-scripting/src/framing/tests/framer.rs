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
