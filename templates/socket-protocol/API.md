# Protocol package API 1

## Component file

A local package is one WebAssembly Component file. It contains exactly one top-level
`intercept-proxy:manifest` custom section whose bytes are the strict API 1 Manifest JSON. The host
validates the Component, Manifest, recursive Schema, and the Manifest-selected WIT world before the
package can be called.

## WIT exports

The versioned WIT contract is `src-tauri/crates/package-runtime/wit/protocol-package.wit`.

- HTTP Decode and Encode use `string`.
- Socket Frame, Decode, and Encode use raw `list<u8>`.
- Document values cross WIT as canonical JSON `string` and are parsed into the host `Document`.
- Display returns untrusted HTML `string`.

The local host calls these exports directly through Wasmtime. It does not use WebSocket, JSON-RPC,
or Base64 for local Hooks.

## Host capabilities

The Component imports WASI plus the versioned Host WebSocket interface. WebSocket supports `ws` and
`wss`, text, binary, receive, and close. A package only uses it when its own implementation needs an
outbound connection; ordinary Hook invocation does not pass through WebSocket.

## Public `/packages` JSON-RPC

Remote debugging packages may initiate a WebSocket connection to `/packages` and send one
`package.register` notification. The fixed methods remain `hooks.upstream.frame|decode|encode`,
`hooks.downstream.frame|decode|encode`, and `document.upstream|downstream.display`.

Only this remote JSON-RPC adapter represents Socket bytes as canonical padded Base64. The Proxy does
not retry, replay, select another package version, or fall back from a failed local Component to a
remote package.
