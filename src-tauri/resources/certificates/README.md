# Fixed test Root CA

`Intercept Proxy` 的受控测试版本在 macOS、Windows 使用同一张 Root CA，便于测试
客户端只内置一个公开信任锚。公开证书的 SHA-256 指纹为：

```text
B4:72:77:A5:8D:81:AD:EB:3C:CE:59:7A:15:58:85:4D:AB:3D:0B:30:AB:CE:15:06:5A:FB:73:33:9B:CB:D7:4C
```

签发私钥随受控测试应用分发，因此可以被提取。该证书链不得用于生产、预生产、
真实商户或任何非隔离环境。普通导出功能只导出公开 `.crt`，不会导出私钥。
