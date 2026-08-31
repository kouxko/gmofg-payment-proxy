# Protocol package API 1

## Archive

A package is one ZIP whose root contains `manifest.json`, `protocol.js`, and `display.js`.
Every additional file must be a package-relative JavaScript module. Paths are case-sensitive and
must not contain wrappers, absolute paths, empty segments, `.` or `..`.

## Manifest

`manifest.json` is strict JSON with `api`, `kind`, `package`, and `document`. Socket packages must
provide upstream and downstream recursive JSON Schema objects. Unknown fields are rejected.

## Local Boa exports

`protocol.js` exports:

- `upstreamFrame` and `downstreamFrame`
- `upstreamDecode` and `downstreamDecode`
- `upstreamEncode` and `downstreamEncode`

`display.js` exports `upstreamDisplay` and `downstreamDisplay`.

Frame receives `{ buffer: Uint8Array }` and returns `need_more`, `complete`, or `reject`.
Decode receives `{ input: Uint8Array }` and returns a JSON Document. Encode receives
`{ originalInput: Uint8Array, document }` and returns `Uint8Array`. Display receives `{ document }`
and returns untrusted HTML text.

The Boa Sidecar evaluates modules and verifies every fixed export before registration. The current
host does not inject Node, filesystem, process, Buffer, fetch, timer, or WebSocket bindings. This is
the current host surface, not a general restriction on Boa default or native capabilities.

## Public `/packages` JSON-RPC

Both local Sidecars and remote package processes initiate a WebSocket connection to `/packages` and
send one `package.register` notification without an `id`; its `params` are the complete Manifest,
and the Proxy sends no response to that notification. Proxy calls use the fixed method names
`hooks.upstream.frame`, `hooks.upstream.decode`, `hooks.upstream.encode`,
`hooks.downstream.frame`, `hooks.downstream.decode`, `hooks.downstream.encode`,
`document.upstream.display`, and `document.downstream.display`.

The public JSON-RPC wire carries Socket binary input and output as canonical padded Base64 text;
`Uint8Array` is only the local Boa export boundary. Every response copies the request's string `id`.
Failures place their stable machine code in `error.data.code`; clients must not depend on the human
`message`. The Proxy does not retry, replay, select another package version, or switch execution paths.
