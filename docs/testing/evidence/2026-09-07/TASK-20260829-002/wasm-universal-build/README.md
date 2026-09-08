# 当前Wasm版本macOS Universal构建

TASK-20260829-002；2026-09-07。本地Universal构建PASS；独立复核APPROVE（P0/P1/P2=0）；父任务人工/分发门禁仍未完成。

## 范围与历史来源

沿用 [Phase18](../../../2026-09-01/TASK-20260829-002/phase18-final-unified-release-automation/README.md) 的Universal App/DMG验收，当前本地运行时按ADR-010/TASK-20260901-001替代旧Boa/Sidecar。原历史产物不覆盖，新用例只证明本次Wasm产品。正式tag/GitHub Release不在本任务范围。

## 输入、环境与步骤

输入源码/配置/脚本/Component/固定Companion资源在resources/build-inputs.tar.gz，保留仓库原始相对路径，文件清单outputs/build-inputs.json；排除任务无关的原未跟踪gmofg_cps/http-finalize目录。外部Cargo/Deno/SDK依赖按锁文件与当前工具链恢复，版本/目标/磁盘/签名身份见outputs。未保存无关全量环境变量或凭据。

先补齐x86_64-apple-darwin Rust目标（target-install.log），再执行 `CARGO_BUILD_JOBS=2 deno task build:macos:universal`。既有入口编译arm64+x86_64、合并App、ad-hoc签名、hdiutil UDZO生成DMG，随后verify-macos-universal检查两架构/CFBundleIdentifier/strict codesign，并只读挂载DMG核验内部App后卸载。实际构建输出build.log，终态build-result.json。任何步骤失败都不得当作Universal成功。

复测需在匹配当前输入的完整checkout使用同一命令；现有脚本会替换src-tauri/target下可再生产bundle，但不安装到/Applications、不改变设备或产品运行配置。实际App ZIP与DMG已保存到outputs/artifacts，清单见outputs/artifact-inventory.json。

## 明确未验收

本机有效codesigning身份0、Developer ID身份0，本轮仅ad-hoc验证，不声称Developer ID正式签名、公证或Gatekeeper分发通过。未执行Windows安装运行、Android验收、外部业务请求、push/CI/发布。协议帧和业务截图N/A（本次仅构建产物）。原任务仍有其他人工环境门禁，不能因本项成功直接归档。

## 本次结果

- 2026-09-07 19:39:01 +08:00，完整构建exit 0；arm64 Release 11m06s、x86_64 Release 10m57s。Universal主程序184503968 bytes，App ZIP 66582148 bytes，DMG 75259890 bytes。
- App与只读挂载DMG内部App：arm64+x86_64、com.interceptproxy.desktop、strict codesign均PASS，DMG已卸载。ad-hoc仅用于本地包验证。hdiutil提示命令弃用警告，实际生成和验证成功；本次未扩大到脚本迁移。
- 1247个实际归档构建输入在完成后逐字节一致；DMG归档副本与构建输出逐字节一致，详情见source-stability.json和artifact-inventory.json。
- 产物独立重放入口：`python3 docs/testing/evidence/2026-09-07/TASK-20260829-002/wasm-universal-build/replay/verify.py`。需macOS自带ditto/lipo/plutil/codesign/hdiutil与已记录Deno版本；脚本解开归档App、复制DMG到临时目录，使用归档的原verify-macos-universal.mjs检查并卸载DMG，最后清理临时目录，不安装App。
- 产物检查输出artifact-checks.json、完整build.log及build-result.json均已保留；独立审查另存outputs/independent-review.md。

独立复核已APPROVE，原始归档重放日志见outputs/archived-replay.log（主Agent额外执行），Reviewer结论见outputs/independent-review.md。Companion复验：从resources/build-inputs.tar.gz读取android-companion.apk，从App ZIP提取同名资源，再以hdiutil只读挂载DMG读取内部App资源，三者直接字节相等（23985398 bytes），最后卸载；结果见outputs/companion-byte-comparison.json。该复验只比较实际资源，不执行APK或修改设备。
