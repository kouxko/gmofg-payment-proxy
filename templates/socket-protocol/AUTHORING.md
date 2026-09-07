# Authoring a Socket protocol package

1. Copy the Rust starter under `iso8583-standard` or generate the same WIT world from another
   Component-capable language.
2. Change the exact identity and recursive schemas in `manifest.json`.
3. Implement both directions of Frame, Decode, Encode, and Display from the versioned WIT contract.
4. Build one `wasm32-wasip2` Component file.
5. Append `manifest.json` as the unique top-level `intercept-proxy:manifest` custom section. In this
   repository, `pnpm build:protocol-packages` owns that packaging step; direct Cargo artifacts are
   not importable until packaged.

Frame receives raw `list<u8>` and returns need-more, complete, or reject. Decode receives raw
`list<u8>` and returns Document JSON. Encode receives the original raw `list<u8>` plus Document JSON
and returns the complete raw frame. Display returns untrusted HTML text.

The Host provides WASI and an optional outbound WebSocket interface. Packages can use `ws` or `wss`
when their protocol implementation requires it; local Hook calls themselves are direct WIT calls.
Remote source-language debugging remains available through `/packages` WebSocket JSON-RPC, where
the remote adapter alone uses Base64 for binary values.

Rules run only at `Proxy -> Server` and `Proxy -> App`. Each direction creates a private working
Document; every rule condition reads its current state, matching actions update it immediately, and
later rules observe earlier changes. The final Document is encoded once.

There is no compatibility mode. A ZIP, core Wasm module, duplicate Manifest, wrong WIT world, or
invalid Component must be corrected and re-imported.
