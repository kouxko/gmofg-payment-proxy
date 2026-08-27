# 复测步骤

```bash
PYTHONPATH=examples/external-packages/nuvei_tango_json \
  python3 -m unittest discover \
  -s examples/external-packages/nuvei_tango_json/tests -v

python3 -m compileall -q \
  examples/external-packages/nuvei_tango_json/nuvei_tango_json \
  examples/external-packages/nuvei_tango_json/tests

```

判定：13 个测试 PASS；日志测试同时断言正常和错误元数据，并断言 Base64、合成敏感值和
`json_preview` 不出现在序列化日志中。
