# Socket protocol package template

The bundled starter is a Rust WebAssembly Component implementing package API 1. Its manifest is
embedded in the top-level `intercept-proxy:manifest` custom section, so the resulting `.wasm` file is
the complete import unit. Local Components run in the Proxy process and do not connect to
`/packages` or use JSON-RPC/Base64 transport.

Build every protocol-package example and template Component from the repository root:

```bash
pnpm build:protocol-packages
```

Artifacts and a machine-readable index are written to `dist/protocol-package-components/`.

See [API.md](API.md), [AUTHORING.md](AUTHORING.md), and the
[ISO 8583 starter](iso8583-standard/README.md).
