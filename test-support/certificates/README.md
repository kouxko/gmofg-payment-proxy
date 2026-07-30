# Unified Test Proxy Root CA

These materials implement the isolated-test-only certificate mode:

- `bundled-payment-server.crt` contains the same PEM certificate bundle as the
  current Payment `app/src/main/assets/server.crt` (the repository copy adds only
  a trailing newline). The desktop app uses its final valid CA as
  the default GMO-FG Server trust anchor and allows an operator-selected bundle
  to replace that default.
- `unified-test-proxy-root-ca.crt` is the public trust anchor that the controlled
  test Payment build/release process appends to `assets/server.crt`. The desktop
  app's certificate page exports this exact PEM certificate as a `.crt`.
- `unified-test-proxy-root-ca-signing-key.TEST-ONLY.txt` is intentionally bundled
  into test Proxy builds so every installation can issue a machine-local server
  leaf certificate with its own key and SAN.

The Root CA subject contains `TEST ONLY`. Its SHA-256 fingerprint is:

```text
E6:0C:7C:71:6A:1A:E9:08:F8:87:8E:4E:98:27:FC:B1:9C:3B:2D:B8:CA:36:15:09:2C:E6:3F:32:94:A1:2B:66
```

The signing key is extractable by design. These files must never be used for
production, pre-production, or real merchant trust. Rotation requires replacing
both resources, updating the pinned fingerprint test, and publishing a new test
Payment build that trusts the replacement public certificate. The desktop export
path exposes only the public certificate and never writes the signing key.
