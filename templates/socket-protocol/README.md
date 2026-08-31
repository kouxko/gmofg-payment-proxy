# Socket protocol package template

The bundled starter is a strict package API 1 ZIP executed by a local Sidecar. The package process
initiates the existing `/packages` WebSocket registration and serves the same fixed JSON-RPC methods
as any remote package.

The ZIP root contains exactly:

- `manifest.json`
- `protocol.js`
- `display.js`

Additional package modules may use package-relative `.js` paths. Directory wrappers, alternate
manifests, configurable entry names, compatibility aliases and runtime fallbacks are rejected.

See [API.md](API.md), [AUTHORING.md](AUTHORING.md), and the
[ISO 8583 starter](iso8583-standard/README.md).
