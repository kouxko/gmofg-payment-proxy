# CPS GitLab 提交与 Merge Request 交付

结果 **PASS_MR_CREATED_VERIFIED**。本地主工作区提交 f92e6674；基于已包含HTTP Finalize的
GitLab develop 58d1d33f 创建独立交付分支，cherry-pick得到 bb0f795b。22个交付文件逐字节
与本地提交一致。使用 `git push -o ci.skip -u gitlab feature/0907-004-cps-checksum` 推送，
通过GitLab API创建MR !4，目标develop；未执行实际合并或CI监控。

MR读取结果、单提交列表、22文件差异列表见 outputs。标题=提交首行，描述=提交正文，
已与 inputs/commit-message.txt 精确比较。MR为opened且无冲突；metadata记录具体SHA和URL。

提交前缓存差异检查发现Java oracle prefix末尾多一个空行，仅删除该空行，无算法或fixture改动；
重新执行原Java oracle，14向量PASS（outputs/post-format-java.log）。
另执行 `cargo test --manifest-path examples/protocol-packages/gmofg_cps/Cargo.toml --locked --doc`，
1项生成绑定doctest通过（outputs/doctest.log）；该测试不替代业务向量。交付版本prefix保存于
resources同名仓库路径，全部22文件最终摘要见 inputs/delivered-file-sha256.json。
父 cps-checksum 档案保存功能验收时的源码/环境/实际Wasm/协议帧；这里记录仅格式的最终差异，
不改写父证据。resources/scoped-commit.patch是本次准确的源码、测试、说明及Wasm二进制提交。

只提交CPS包及其Host/HTTP回归22文件。共享工作区中的任务索引、其他任务和本地证据不纳入
产品提交；本任务及其父证据在本地docs体系保持稳定链接。

复核：使用有该GitLab项目读取权限的客户端读取MR !4及其commits/diffs，与归档JSON和
commit-message比较；检出bb0f795b后对22路径计算SHA-256，与inputs清单比较。
无需重新创建MR、触发CI或再次发送代码。功能复测沿用 ../cps-checksum/replay/run.py。
