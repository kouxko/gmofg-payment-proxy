# ISO 8583:1987 ASCII starter

This directory is the Rust source of the bundled single-file
`iso8583-ascii-standard@1.0.0` WebAssembly Component.

- `manifest.json` owns the exact identity and upstream/downstream JSON schemas.
- `src/lib.rs` implements the two-byte big-endian frame header, directional WIT exports, and
  untrusted HTML display output.
- `Cargo.toml` builds the package for `wasm32-wasip2`; the repository packaging step appends the
  manifest as the top-level `intercept-proxy:manifest` custom section.

The current starter decodes the four-character message type and preserves all other original frame
bytes. Changing `message_type` rewrites those four ASCII bytes and retains the original length and
payload. Extend the schema and codec together when adding data elements; do not add a second
manifest or alternate execution entry.

Files under `samples/` are authoring vectors and are not included in the compiled Component.

Run `cargo test --locked --manifest-path templates/socket-protocol/iso8583-standard/Cargo.toml` for
the host-side codec tests. From the repository root, run `pnpm build:protocol-packages`; import
`dist/protocol-package-components/intercept-proxy-iso8583-ascii-standard-component.wasm`.
The direct Cargo artifact does not yet contain the required top-level Manifest.
