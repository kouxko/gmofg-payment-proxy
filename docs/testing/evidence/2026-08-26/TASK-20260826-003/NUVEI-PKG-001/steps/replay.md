# 复测步骤

从仓库根目录执行：

```bash
PYTHONPATH=examples/external-packages/nuvei_tango_json \
  python3 -m unittest discover \
  -s examples/external-packages/nuvei_tango_json/tests -v

python3 -m compileall -q \
  examples/external-packages/nuvei_tango_json/nuvei_tango_json \
  examples/external-packages/nuvei_tango_json/tests

wheel_dir=$(mktemp -d)
python3 -m pip wheel --no-deps --wheel-dir "$wheel_dir" \
  examples/external-packages/nuvei_tango_json
```

判定：12 个测试全部 PASS；compileall 退出码为 0；生成
`nuvei_tango_json-1.0.0-py3-none-any.whl`。

不要使用真实授权报文作为复测输入，不要联网重放。
