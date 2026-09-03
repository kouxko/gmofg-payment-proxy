# 复测步骤

1. 启动 `Payment DLL` Listener。
2. 使用仓库固定 Root 验证下游 TLS，并把绝对 HTTP URI 指向本机不可用端口；若 Mock 未命中，连接应失败，不能误发到真实上游：

```bash
curl --http1.1 \
  --cacert src-tauri/resources/certificates/intercept-proxy-test-root-ca.crt \
  -D - -o - -X POST 'https://127.0.0.1:8080/' \
  --request-target 'http://127.0.0.1:9/' \
  -H 'Host: 127.0.0.1:9' \
  -H 'Content-Type: application/json' \
  --data-binary @<request-body-file>
```

3. 预期 HTTP 200、`Content-Length: 118` 和 `outputs/local-replay-response.http` 中的 D48 正文。
4. MCP 调用 `workspace_rule_list` 和 `http_capture_get`，预期目标规则 `hit_count` 增加且 capture 的
   `matched_rule_ids` 只包含 `b3996e35-0c57-4971-9a73-74b809d33aee`。
5. 停止 Listener，恢复测试前运行状态。
