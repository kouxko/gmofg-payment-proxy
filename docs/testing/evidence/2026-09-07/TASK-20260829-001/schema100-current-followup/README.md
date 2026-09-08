# Schema100 当前合同续验

TASK-20260829-001，2026-09-07。当前本地验证PASS，独立整体复审APPROVE，见[independent-review.md](independent-review.md)。

## 合同与原测试复用

原任务和原用例 [DB-SCHEMA-100-001](../../../2026-08-29/TASK-20260829-001/DB-SCHEMA-100-001/README.md) 已验证合法<100清理重建、=100保留、未来/损坏标记拒绝、WAL/外键/并发、Release App和实际UI。后续TASK-20260829-002最终采用启动所有权下清理旧主库/WAL/SHM；其曾出现的中间态“所有旧版本只拒绝”不作为当前合同。

本次不改数据库代码，使用当前权威源码及现有测试复验。输入是测试生成的旧Schema、未来/坏marker、WAL/外键/并发和Host双启动隔离数据库，测试源、运行时fixture和checker直接输入分别随resources/current-source.tar.gz、runtime-fixture-source.tar.gz、checker-source.tar.gz归档；不操作用户正式数据库。

## 实际命令与结果

1. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib sqlite::`：88通过、0失败、0忽略；完整输出outputs/sqlite-tests.log。
2. `deno task test:task-20260829-002:phase17`：14项release checker通过；Infrastructure旧库清理exact1/1、Host旧库清理exact1/1、optimized Release双Host启动持久化exact1/1通过；完整输出outputs/release-startup.log。checker先验证exact测试唯一发现，零测试不能算成功。
3. 测试前归档SQLite/Host测试与Cargo/checker输入，执行后逐文件字节比较不变；outputs/source-stability.json。
4. 已有GitHub CI 34025792017 的 Windows Verify job当前回读SUCCESS，当前相关SQLite/Host/Cargo文件与该run的fa4e154f逐字相同（outputs/windows-source-comparison.json）。完整run因frontend coverage失败，不报告完整CI成功。另一个quick run34020524291已完成Android/Windows构建；原始回读均在outputs。

## 环境、复测与限制

从本目录运行 `python3 replay/run.py`；需当前仓库、Rust/Cargo1.98.0与Deno及项目既有构建依赖，版本见outputs/rust.txt、cargo.txt、platform.txt。资源路径、来源、用途和必要性见metadata.json，归档保留仓库原始相对路径。复测拒绝被测源与归档不同，避免把后续源码当成历史复验。测试自动管理临时目录和Host关闭，无需改设备或生产数据库。

原实际App/UI验收保留为历史已验证，本轮没有新UI验收；数据库任务没有UI变更。协议报文/截图N/A，本轮push/新CI/发布NOT_RUN。Windows证据只覆盖实际成功job，不扩大到失败的完整CI或Windows安装运行。

## 运行时文件资源补齐

优化双启动用例会临时编译ISO8583 Component。为保存实际读入的文件，保持测试源码不变，对已构建的Release测试二进制设置CARGO为 `replay/capture-cargo.py`，包装器调用原cargo并在临时目录删除前复制原始Wasm；exact测试再次1/1通过。命令/环境在outputs/fixture-capture-command.json，输出在fixture-capture-repeat.log。该重复执行仅补齐初次被临时目录清理的资源缺口。

resources/runtime-fixture-source.tar.gz保存实际模板、Manifest、Cargo.lock、WIT和SQLite源码合同读取的adapter；resources/generated-component保存实际被测试读取的原始Wasm，Manifest在Host测试中按明确CustomSection逻辑追加（代码归档保留）。三份源归档共51个文件逐字节一致。测试复测默认通过原命令生成临时Component；若需要重新捕获文件，DB100_CAPTURE_DIR必须指向新的空目录，包装器用排他写入拒绝覆盖旧证据。

## 审查修正

独立审查发现初稿缺少checker直接读取的8个Tauri/Host/Infrastructure入口与架构文件（P1）。已补入resources/checker-source.tar.gz；这些文件相对本轮测试起点caa36022无差异；replay已对第三份归档执行同样的逐字节检查。未改变业务源或测试输入，不重复耗时测试；三份源归档51文件校验通过，独立复审APPROVE，残留发现0。
