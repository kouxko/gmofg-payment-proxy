# Authoring a Socket Sidecar package

1. Copy the three root files from `iso8583-standard`.
2. Change the exact identity and recursive schemas in `manifest.json`.
3. Implement both directions of Frame, Decode and Encode in `protocol.js`.
4. Implement both Display exports in `display.js`.
5. ZIP the file contents at the archive root, without a containing directory.

Keep framing independent per direction. `complete.consumedBytes` must be positive and no greater
than the current buffer length. Decode returns natural JSON values. Encode must preserve unchanged
bytes when the Document is unchanged and return the complete frame when it changes.

JavaScript modules may import only relative package modules. The current Boa host does not inject
Node, filesystem, process, Buffer, fetch, timer, or WebSocket bindings; this statement describes the
host surface and does not redefine Boa default/native capabilities. The local Sidecar itself
initiates `/packages` registration with `package.register`; the Proxy never opens a second package
protocol. Local exports receive `Uint8Array`, while public `/packages` JSON-RPC represents bytes as
canonical padded Base64 text.

Rules run only at `Proxy -> Server` and `Proxy -> App`. Each direction creates a private working
Document; every rule condition reads its current state, matching actions update it immediately, and
later rules observe earlier changes. The final Document is encoded once. Package code must not expect
a rule callback at Reader/Decode/Display boundaries.

There is no compatibility mode. An archive that does not match package API 1 must be corrected and
re-imported.
