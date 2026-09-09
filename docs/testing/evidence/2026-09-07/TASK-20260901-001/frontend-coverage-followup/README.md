# 前端覆盖率与归档编译边界续办

TASK-20260901-001 WPC-14；2026-09-07。最终稳定配置验证PASS，独立局部审查APPROVE，见[independent-review.md](independent-review.md)。73文件560测试全通过；Socket分支302/325=92.92%，原门槛90%保持。

## 原因与范围

历史CI34025792017因Socket UI分支覆盖率283/325=87.07%低于90%失败；当前listeners/vitest配置与该run逐字无差异（outputs/historical-source-comparison.json），原coverage-reports下载至resources/ci-coverage。仅补Socket model和processing card测试，不改变生产行为/门槛。

补测覆盖目录fail-closed、推荐包合同、单侧TLS更新和禁止隐式启用TLS、不可用目录、本机应答无包、精确包版本替换及运行生命周期显示。实际测试包含用户交互与输出状态断言。

完整typecheck还发现历史证据outputs/reviews中5处TS2307。tsconfig原本只排除evidence的resources，漏掉其他归档目录；只将排除边界改为docs/testing/evidence/**，strict设置及应用输入不变。修复后typecheck PASS；tsc --listFilesOnly确认208份src TypeScript全部纳入、证据输入0，见outputs/typecheck-input-boundary.json。

## 过程中的失败与最终验收边界

- outputs/before.log：并行Release编译时默认worker的首次全覆盖出现超时后中止，不是完整coverage结果，SIGINT另触发Deno退出码NaN错误。最终执行仅将测试worker数限制为2，不更改timeout/阈值或断言。
- outputs/typecheck.log：真实归档范围漏排导致的失败；typecheck-after-exclude.log：修复后通过。
- outputs/full-final.log：73文件560测试通过，Socket302/325=92.92%；因tsconfig在结束前18秒变更，此轮不作为最终稳定配置验收。first-complete-coverage.tar.gz保留其完整report。
- outputs/stable-final.log及stable-final-result.json：最终冻结源码/配置下73文件560测试PASS；源码归档逐字不变，完整报告stable-coverage-report.tar.gz，比较见comparison.json。
- 子Agent定向2文件26测试及ESLint已通过；误触发deno task check后在bindings编译阶段中止，确认无遗留进程/生成源码修改，该中止不作为验收成功。

## 输入、资源和复测

资源来源/用途/必需性见metadata.json。before-source保留原测试输入，frontend-source归档最终全部src和配置；外部运行依赖通过package.json/deno.lock、Deno2.9.6和仓库既有安装流程重建。归档源码使用tar.gz，避免污染应用编译。

从本目录执行 `python3 replay/run.py`：先核对归档与当前源逐字一致，再运行完整coverage（maxWorkers=2）、typecheck和改动文件ESLint。测试期间应冻结src及配置；输出进入调用终端，不改历史证据。前后覆盖数与实际测试总数保存到outputs/comparison.json。

协议原始帧/网络业务报文/原生App截图N/A（无运行合同变更）；组件交互自动化不等于原生或远端验收。本轮未push、未触发新CI、未发布；WPC-12远端HTTP和WPC-14主分支/tag Release仍未完成。
