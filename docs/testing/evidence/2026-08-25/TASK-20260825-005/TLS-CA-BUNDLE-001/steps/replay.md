# TLS-CA-BUNDLE-001 复测步骤

从仓库根目录执行。

## 1. 设置用例目录

```bash
case_dir='docs/testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001'
mkdir -p "$case_dir/outputs"
```

## 2. 使用 OpenSSL 规范化并组合 Bundle

```bash
{
  openssl x509 -in "$case_dir/resources/sub.pem" -outform PEM
  openssl x509 -in "$case_dir/resources/DigiCertCA.pem" -outform PEM
} > "$case_dir/outputs/FirstData-trust-chain.pem"
```

## 3. 查看 Bundle 成员

```bash
openssl crl2pkcs7 \
  -nocrl \
  -certfile "$case_dir/outputs/FirstData-trust-chain.pem" |
openssl pkcs7 -print_certs -noout
```

输出必须包含：

```text
First Data Latvia Internal CA
First Data Baltics root CA
```

## 4. 验证证书链关系

```bash
openssl verify \
  -CAfile "$case_dir/resources/DigiCertCA.pem" \
  "$case_dir/resources/sub.pem"
```

预期：

```text
sub.pem: OK
```

## 5. 执行实现后的 Rust 定向测试

测试过滤器由实现阶段按最终测试名补充并写回本文件：

```bash
cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-infrastructure listener_certificates

cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-runtime socket_relay::tests::tls
```

## 6. OpenSSL 直接握手基线

```bash
openssl s_client \
  -connect 195.160.171.102:63002 \
  -CAfile "$case_dir/outputs/FirstData-trust-chain.pem" \
  -showcerts \
  -verify_return_error
```

保存完整 stdout/stderr，并分别记录 TCP、TLS、证书链、hostname/IP 和客户端证书请求结果。

## 7. Proxy 实际验收

实现后在 Proxy 中导入：

```text
outputs/FirstData-trust-chain.pem
```

配置 Socket Listener 上游：

```text
Host: 195.160.171.102
Port: 63002
Security: TLS
Trust source: Explicit PEM Bundle
```

保存 Listener 配置、运行快照、`server_trust_count`、握手日志和最终分层结论到新的正式执行证据目录。
