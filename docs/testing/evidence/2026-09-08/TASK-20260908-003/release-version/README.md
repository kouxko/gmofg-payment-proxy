# 1.0.1 发布版本一致性

目的：只验证本次发布分支的版本配置、release/develop历史整合及MR就绪；不声称进行了新的全平台构建、设备覆盖安装或业务验收。
基线：develop bb47b0eb（已包含MR!8），合入release 10b3b576；预合并产品树不变，仅补齐release独有祖先。当前没有未解决冲突。

从仓库根执行：

```bash
python3 docs/testing/evidence/2026-09-08/TASK-20260908-003/release-version/replay/check_versions.py --repo .
cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --locked --offline --no-deps
deno test -A scripts/deno-toolchain-contract.test.mjs scripts/build-macos-universal.test.mjs
deno lint scripts/build-macos-universal.mjs
deno fmt --check package.json src-tauri/tauri.conf.json scripts/build-macos-universal.mjs
git diff --check
```

实际结果：Android主APK已从develop继承1.0.1/code101；Proxy前端、Tauri、Cargo工作区均1.0.1，DMG文件名1.0.1。13个内部Cargo包的lock条目只改变version，所有外部依赖保持。5项现有配置/打包合同测试通过，脚本lint与格式通过。Tauri CLI info实际读取当前配置并退出0；它仅显示环境/配置，工作区未安装前端依赖，未用于声称构建完成。

首次全依赖offline metadata因未缓存Linux memfd0.6.5失败，保留stderr和attempt.json；随后采用本范围所需的locked/offline/no-deps元数据检查成功，不隐藏失败、不因此升级/下载外部依赖。

resources保存实际版本输入、派生lock、原baseline lock、当前脚本和两个测试输入；replay/check_versions.py验证当前工作树，输出实际版本、内部包和源SHA。outputs保存原始stdout/stderr及比较。没有网络业务输入、Frame、截图或支付请求，均N/A（本次仅配置与交付准备）。CI保持[skip ci]，MR远端结果后续补充。
