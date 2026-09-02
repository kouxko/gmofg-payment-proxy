# 复测步骤

1. 自动化验证：

   ```bash
   PYTHONPATH=examples/external-packages/nuvei_tango_json:examples/external-packages/nuvei_tango_json/tests \
     python3 -m unittest discover -s examples/external-packages/nuvei_tango_json/tests -v
   python3 -m compileall -q \
     examples/external-packages/nuvei_tango_json/nuvei_tango_json \
     examples/external-packages/nuvei_tango_json/tests
   ```

2. 在受控网络连接远端 Proxy 外部包入口：

   ```bash
   EXTERNAL_PACKAGE_URL=ws://10.0.28.85:8765/packages \
   NUVEI_TANGO_ALLOW_INSECURE_REMOTE_WS=1 \
   PYTHONPATH=examples/external-packages/nuvei_tango_json \
     python3 -m nuvei_tango_json.main
   ```

3. Proxy 选择 `nuvei-tango-json@1.0.0`，启动目标为 `tangodev.nuvei.com:9081` 的 Socket Listener，
   由授权测试 App 发起一笔测试交易。

4. PASS 判定：同一 Exchange 上行和下行依次出现 `split_frame`、`decrypt_message`、
   `render_message`、`encrypt_message` 的 `outcome=ok`；split 消费字节数和 encode 输出字节数分别等于
   对应输入字节数；无 `InvalidResponse`、`DECODE_FAILED` 或外部包断连。

真实交易报文、Base64、PAN 和 Track2 不写入复测记录。
