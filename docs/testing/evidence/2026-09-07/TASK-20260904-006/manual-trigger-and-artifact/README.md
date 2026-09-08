# 手动构建入口与联合产物核验

TASK-20260904-006；2026-09-07续办。当前验证通过，独立整体审查APPROVE（P0/P1/P2=0），见[independent-review.md](independent-review.md)。

最终入口只允许 workflow_dispatch。2026-09-04保留的一次性push分支未被移除，原测试继续强制临时状态。先将合同测试恢复为手动入口并取得RED（2通过/1失败），再仅删除workflow的4行触发器/注释，GREEN 3/3。两个job、依赖、缓存、编译、artifact布局均不变。

## 输入、结果与来源

- resources/red：实际RED测试与原workflow；resources/green：实际GREEN测试、两个workflow和依赖声明。
- outputs/red.log、green.log、fmt.log、lint-pinned.log、diff.log：真实输出。
- outputs/yaml-final.log：实际解析只有workflow_dispatch、两个job且Windows依赖Android。首次裸包导入失败保留在yaml.log，改用已安装的本地js-yaml模块解决，未新增依赖。
- outputs/run-34020524291.json：成功quick run当前API回读；resources/artifact：本次从该run重新下载的原始exe/APK；outputs/artifacts.json记录来源与有效性。
- outputs/artifact-check.json：x86-64 PE32+、APK ZIP CRC PASS；仅intercept-proxy.exe和resources/android-companion.apk，大小81,017,344与23,985,398字节。
- outputs/workflow-comparison.json：当前workflow与成功run除临时push块外逐字一致，沿用历史job执行证据，不声称本分支已远程构建。
- outputs/run-34025792017.json：完整CI总体失败（coverage），Windows Verify与Android成功，不当成完整CI PASS。

## 环境、步骤与复测

从本目录执行 `python3 replay/run.py`。需要Deno（版本见outputs/environment.log）、Python3；归档测试只用Deno内建Node兼容模块，不需仓库或网络。完整静态检查从仓库运行 `deno fmt --check scripts/windows-build-only-workflow.test.mjs` 与 `deno run -A node_modules/eslint/bin/eslint.js scripts/windows-build-only-workflow.test.mjs --max-warnings=0`，依赖按resources/green的声明安装。远程刷新可执行 `gh run view 34020524291 -R kouxko/gmofg-payment-proxy --json databaseId,headSha,status,conclusion,jobs`；仅只读查询，不触发CI。

生产源码未变。旧run head=fa4e154f，当前分支起点=caa36022（develop），不混用为同一提交证明。当前测试期间workflow/test冻结并保存原文快照。协议帧/业务报文/UI截图N/A；Windows启动NOT_RUN；本轮新CI/push/release NOT_RUN。
