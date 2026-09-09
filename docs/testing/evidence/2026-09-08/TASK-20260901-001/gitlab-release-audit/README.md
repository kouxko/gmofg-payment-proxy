# GitLab 已发布制品只读核对

TASK-20260901-001 / gitlab-release-audit / PARTIAL_EXISTING_RELEASE_VERIFIED

当前项目1023、默认master、Release v1.0.0指向a146e75c，来自caa36022；其产品源码与本工作分支初始版本一致。API原文见 outputs/project.json、release.json、pipelines.json、jobs-34120.json。既有34120流水线Android与Windows build-only均SUCCESS，不包含完整质量/发布门禁。34145也已SUCCESS，但属于后续CPS功能分支，不能替代当前任务或v1.0.0全CI。

四个Release链接已真实下载并保存在 resources/：Windows便携ZIP、NSIS EXE、MSI以及Android APK。便携ZIP/APK全entry CRC通过；PE机器类型AMD64、MSI结构x64；便携包内APK与单独APK逐字节一致；Android签名通过且包含四ABI。完整文件名、bytes、URL、archive entries及结果在 outputs/report.json、cross-asset-check.json、apk-signature.txt。Windows安装执行未在当前macOS执行；不能从文件结构推导真实安装或运行通过。

Release目前没有macOS资产。已完成的本地Universal App ZIP/DMG保存在 `docs/testing/evidence/2026-09-07/TASK-20260829-002/wasm-universal-build/outputs/artifacts/`，但未在本轮上传；macOS构建/验证结果见该父任务证据。本轮新增cap及coverage修复也尚未推送到GitLab。WPC-14继续未完成，不以已有4个下载文件替代全部交付。

复测：仓库根目录执行 `python3 docs/testing/evidence/2026-09-08/TASK-20260901-001/gitlab-release-audit/replay/audit.py`。需要当前GitLab只读访问凭据、git credential helper、Python标准库、file；签名检查另使用本机Android SDK build-tools/36.1.0/apksigner。脚本仅GET及下载，凭据只在进程内使用，不写入证据。复测请使用新证据目录，勿覆盖本次已归档结果。

N/A：无新增请求报文/业务、无真实Windows安装、无新CI、无push/tag/Release写入。此用例只证明当前已有制品与远端状态。
