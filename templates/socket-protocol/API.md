# Protocol package API 1

## Archive

A package is one ZIP whose root contains `manifest.json`, `protocol.js`, and `display.js`.
Every additional file must be a package-relative JavaScript module. Paths are case-sensitive and
must not contain wrappers, absolute paths, empty segments, `.` or `..`.

## Manifest

`manifest.json` is strict JSON with `api`, `kind`, `package`, and `document`. Socket packages must
provide upstream and downstream recursive JSON Schema objects. Unknown fields are rejected.

## Fixed exports

`protocol.js` exports:

- `upstreamFrame` and `downstreamFrame`
- `upstreamDecode` and `downstreamDecode`
- `upstreamEncode` and `downstreamEncode`

`display.js` exports `upstreamDisplay` and `downstreamDisplay`.

Frame receives `{ buffer: Uint8Array }` and returns `need_more`, `complete`, or `reject`.
Decode receives `{ input: Uint8Array }` and returns a JSON Document. Encode receives
`{ originalInput: Uint8Array, document }` and returns `Uint8Array`. Display receives `{ document }`
and returns untrusted HTML text.

The Sidecar evaluates modules and verifies every fixed export before registration. Hook failures are
returned as typed JSON-RPC failures; the Proxy does not retry, replay or switch execution paths.
