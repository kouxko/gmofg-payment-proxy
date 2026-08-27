# 复测步骤

从仓库根目录执行。

```bash
BUNDLE=docs/testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/outputs/FirstData-trust-chain.pem
ROOT_CA=docs/testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/resources/DigiCertCA.pem
INTERMEDIATE_CA=docs/testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/resources/sub.pem

openssl crl2pkcs7 -nocrl -certfile "$BUNDLE" |
  openssl pkcs7 -print_certs -noout
openssl verify -CAfile "$ROOT_CA" "$INTERMEDIATE_CA"

cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-infrastructure upstream_ca_bundle_ -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-infrastructure listener_certificates -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-infrastructure environment_configuration_validation -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-runtime socket_relay::tests::tls -- --nocapture

openssl s_client \
  -connect 195.160.171.102:63002 \
  -CAfile "$BUNDLE" \
  -showcerts \
  -verify_return_error \
  -brief </dev/null
```

判定：Bundle 列出两张证书；Intermediate 验证为 OK；全部定向测试通过；真实 TLS 握手
显示证书验证为 OK。最后一个命令只握手并立即结束，不发送业务报文。
