# 测试结果摘要

```text
cargo test -p intercept-proxy-domain --all-targets --all-features
70 + 15 + 6 + 9 = 100 passed, 0 failed

cargo test -p intercept-proxy-application (targeted model and factory)
6 + 4 = 10 passed, 0 failed

cargo test -p intercept-proxy-infrastructure phase10_http_pipeline_tests
15 passed, 0 failed

deno task test
64 files passed, 551 tests passed

deno task test:ui-contracts
30 files passed, 303 tests passed

deno task typecheck / lint / scan:architecture / check:bindings
PASS

cargo fmt --check / cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS
```

当前 HEAD 可独立复现但与本次 Document 通配符无关的基线失败：

```text
requirements_tests::settings_lifecycle::rule_editor_capabilities_are_stage_exact_and_rust_owned
left: 13, right: 12

requirements_tests::unified_rules::unified_save_rejects_every_invalid_http_runtime_shape_without_persistence
ProxyToApp MockResponse 实际按当前 capability 被接受，但旧测试仍 unwrap_err
```

源码尺寸仍被当前 HEAD 的 7 个既有文件阻断；本次触及的最大文件为 494 行。
