# Socket resource limit lane

## Confirmed observer defect

`BoundedSocketConnectionObserver::with_limits` treated the retained event count as an eager allocation size. The RED test constructed a queue with `usize::MAX` event capacity and a small logical byte budget. The previous implementation panicked with `capacity overflow` before any event was recorded.

The implementation now creates an empty `VecDeque`. The event-count limit still controls retained length and the logical-byte budget still evicts the oldest event. The GREEN test records two fixed-size events with a one-event byte budget and verifies the second event replaces the first and increments the eviction counter.

## Commands

RED:

`CARGO_TARGET_DIR=src-tauri/target cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime bounded_retention_does_not_preallocate_the_event_capacity -- --nocapture`

GREEN:

`CARGO_TARGET_DIR=src-tauri/target cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime bounded_retention -- --nocapture`

## Read chunk contract mapping

The product authority should be one constant beside `DEFAULT_SOCKET_READ_CHUNK_BYTES` in the domain socket topology. Domain workspace validation and application candidate validation should consume that constant. Exchange already re-exports domain capability contracts to the transport runtime, so it can re-export the same constant and Proxy config validation can consume it without a new crate dependency or duplicated numeric authority.

The concrete maximum remains unimplemented until the user confirms it. `vec![0; read_chunk_bytes]` is safe after a small absolute product maximum is enforced before service construction. Fallible reservation is useful for arbitrary direct runtime values, but it is not a replacement for the product contract because a large allocation can be representable and still exhaust physical memory.
