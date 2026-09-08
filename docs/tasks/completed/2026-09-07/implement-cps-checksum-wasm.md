# TASK-20260907-004：实现 CPS checksum HTTP Wasm 包

- 任务 ID：TASK-20260907-004
- 状态：已完成
- 任务日期：2026-09-07
- 创建时间：2026-09-07 17:11:19 +08:00
- 开始时间：2026-09-07 17:11:19 +08:00（调查/执行准备）
- 最后更新时间：2026-09-07 18:12:25 +08:00
- 完成时间：2026-09-07 18:12:25 +08:00
- 创建路径：`docs/tasks/pending/2026-09-07/implement-cps-checksum-wasm.md`
- 归档路径：`docs/tasks/completed/2026-09-07/implement-cps-checksum-wasm.md`
- 任务优先级：高
- 优先级理由：涉及远端构建交付或协议/签名及跨模块验收，执行完整适用流程。
- 关键词：HTTP、Wasm、Finalize、CPS、Windows、GitLab

## 背景、目标和范围

实现 CPS downstream Decode/Display/Encode/Finalize 完整链；按当前 CPS 源码签名合同生成可导入 Wasm Component、说明和回归向量。

## 需求确认记录

用户要求“然后实现cps checksum wasm包”；追加“downstream得 decode display encode还是需要一起实现的哈 你看源代码能实现的”。恢复已从 TASK-20260907-001 移出的 CPS 包范围，Proxy Finalize 已完成。

仅授权的现有 Windows 构建可触发 CI。CPS 是本地实现，不因前一项交付授权而自动提交、推送、创建 MR、安装到运行 App 或触发新的 CI。不修改 CPS Android 源码或真实支付系统。

## 需求就绪

已通过：具体就绪时间及模型、接口、错误合同见后文“就绪核验与实现合同”；下行使用 App 原请求 URL token 重签 data.sign，业务不适用完整输入原样返回。

## 当前事实、历史和原因

当前已验证：TASK-20260907-001 已实现通用 Finalize；GitLab MR !3 的单独提交为 183bccfb，目标 develop，无冲突。历史 Windows 产物仅供流程参考，不作为本任务结果。CPS 历史方案须由当前 /Users/codin/Code/jp_gmofg_cps 源码重验。新增能力/构建交付不直接等同缺陷修复；适用现象、复现、候选原因、根因在发现实际故障时追加，暂 N/A。

## 最小改动与最优设计

优先复用现有构建模式与 HTTP Component/WIT/JSON/Finalize 边界，不增加第二套 Proxy Pipeline、兼容回退或未约定默认成功。Windows 只调用现有流程；CPS 使用独立业务包承担字段/签名，Host 保持通用。源合同核验后细化文件所有权、错误模型和向量。

## 小任务与计划

1. 核对合同与输入、保存来源；主 Agent 协调，独立源码调查可并行。
2. 就绪后执行构建或包实现，分配不重叠文件；不在任务锁中运行构建。
3. 定向验证、实际产物和资源快照；全部实现结束后整体独立审查。
4. 同步文档、保存证据并按完成日期归档。小任务不额外开启审查，整批集成后执行一次整体审查。

## 验收标准

源码/Java 对照向量、组件构建加载、Decode→规则修改→Encode→Finalize→验证签名、异常和不适用输入保持、HTML安全展示；独立审查和可复测资源归档。

## 文档与测试计划

任务、docs入口/完成索引/证据索引按锁更新。CPS 包需 README、实际 Manifest/构建命令/字段算法与限制；Windows 保持既有操作文档，差异发现后再修正。正式用例存于 `docs/testing/evidence/2026-09-07/TASK-20260907-004/`，含环境、输入、资源、预期实际、日志、复测和清理。不适用的真实设备、生产 TLS、真实支付、UI和跨平台运行标记 N/A 或 NOT_RUN，不生成占位证据。

## 实施与完成记录

- 2026-09-07 17:11:19 +08:00：登记；尚无构建或包验收结论。
- 修改文件：本任务文档与 docs/README.md 登记入口。
- 附加文件、测试结果、CI情况、审查结果、完成总结：执行后追加，当前未完成。

## 就绪核验与实现合同（2026-09-07）

当前 CPS 源码 db99af6fde2de3f8ae8f29777e19aa45d920c3f9 已核验。用户要求的本次业务范围为完整 downstream Decode/Display/Encode 与响应 checksum Finalize，覆盖 pay/confirm/refund/reverse 各新旧/生产/stub路径；沿原任务S4下行响应重签，不增加请求签名重写。HTTP WIT必需的upstream Decode/Display/Encode同步提供JSON处理。

- Decode：有效JSON作为同结构Document，保留未知字段；空或非JSON正文作为原文string Document，落实已确认D6的业务不适用原样通过。Encode：未变Document保留原始Body字节；改动后按JSON或原文string类型输出；非法Document报PackageError。
- Display：转义后的完整JSON/原文展示，覆盖meta/data/result所有输入字段；不伪造最终签名状态或业务成功。
- Finalize：只处理源码四类POST路径与2xx响应，输入为Host五字段JSON。路径末段百分号解码一次恢复原App token，保留补空格和字面加号；不根据规则修改后的金额重新生成token。
- 对四类Response的meta/data/result按Java具体模型字段投影计算sign；继承字段纳入、未知字段不签、sign和shadow排除、null和int0排除，空字符串保留，值内ASCII空格删除，token不trim。MD5输入UTF-8，输出大写hex，写回data.sign。Android官方Charset文档确认平台默认UTF-8：https://developer.android.com/reference/java/nio/charset/Charset。
- 源码中的请求sign存在，但不在本次原S4响应重签及最新downstream交付目标内。非2xx在CPS中走ResponseWithBaseData错误解析且不验签，Finalize不改。
- 缺token/sign、未知接口、非法Body/必要模型结构或类型不适用均按此前D6完整Finalize输入原样返回；此处不同于五字段封装损坏、非法Document或Wasm执行错误，后者明确报错。子Agent提出的“已知接口无效类型一律Host失败”未采用，因为D6已明确业务缺数据跳过。
- 只改Body中的data.sign，不改变url/method/code/header；Host现有Finalize完成实际framing。sign本来已正确时保持完整输入不重新格式化。

最小与最优方案一致：独立 `examples/protocol-packages/gmofg_cps` HTTP Component，使用已存在库serde/serde_json/wit-bindgen及仓库已锁定md-5 0.10.6（复用现有加密实现，不手写另一套MD5、不引入新的第三方库种类），不修改Host公共合同。模型投影与HTTP/codec/展示职责分文件。JSON保留顺序和数字文本以贴近Gson标量转换。

就绪检查PASS，进入实现时间：2026-09-07 17:25:16 +08:00。目标、范围、输入输出、状态/错误、源码样例和逐向量验收均已明确，无影响当前下行实现方向的未确认项。

分工：root拥有包Cargo/Manifest/lib/codec/display/signing/URL/build/README、Rust单元与Host组件测试；cps_signing_contract_v2拥有 tests/java_reference/** 和 tests/fixtures/java-signatures.json，提供原算法/模型Java oracle与来源SHA；接口向量数组[{id,operation,body,token,canonical,sign}]，body为原JSON文本，负例expect_error=true。互不修改共享文件；全部实现终态后整体审查。

父证据：TASK-20260907-001 / http-finalize / docs/testing/evidence/2026-09-07/TASK-20260907-001/http-finalize；复用现有WIT、Local Wasm和真实HTTP测试结构，本次增加CPS实际包与Java签名预期，不覆盖原档案。

## 实现与联调追加

- 2026-09-07 17:45:35 +08:00：用户追加“wasm本机构建出来给我”；已生成本地实际 Component 并提供文件路径。原生 8 测试、1 doctest、Java 14 向量与实际 Host 六个 codec/export、下行 Finalize 测试通过；最终稳定状态复测与 HTTP wire、整体审查、证据归档继续。
- Host 联调发现 Document 数字为 f64，100 转成100.0使 Value严格相等判定误判为修改；已按现有 json_pretty 的递归数字等价比较修复未变正文重写，Host no-op 原文断言和金额编辑重签均通过。影响只限本包 Encode，未修改领域数字合同。
- Java 合成向量中 tab/newline 原先作为原始控制字符写入 JSON，Gson 宽松接受但标准 JSON 无效。修正 oracle 源文本转义并重新生成向量，所有原 canonical/sign 未改变；14 条标准 JSON 均解析通过。
- cps_signing_contract_v2 在 oracle 完成后承担独立新 cps_checksum_wire.rs 和父 mod 登记；root 保持包及 Host 测试所有权。整体审查等待这批实现全部终态并集成后执行。

## 需求变更：数字词法边界与交付授权

- 2026-09-07 18:04:34 +08:00：用户明确“提交 创建merge请求到develop 合并信息为提交内容”，并明确“写法1e2、1E+2和-0不要考虑了 真实环境不会出现”。授权本任务提交、推送到GitLab并创建目标develop的MR，标题/描述与提交内容对应；未要求实际合并或新增CI，提交使用[skip ci]并跳过push CI。
- 初审发现：Gson String字段遇到上述合法JSON数字词法时保留字面量，而serde_json规范化数字表示，会使签名不同。原P1有效，尚未实施RawValue修复。用户明确缩小支持范围，以上场景不作为本任务验收输入；保留当前实现与14条已确认向量，不声称已修复被排除场景。
- 受影响验收项：不再要求以上数字词法与Gson完全等价；其余四模型、已覆盖标量转换、原token、Decode/Display/Encode/Finalize、字段保留和D6保持原合同。新验收为在此明确范围内通过Java/Host/HTTP及审查。包README同步限制。需求就绪重新检查PASS，没有影响当前支持范围的未确认事项。

## 最终验收与文档同步

- 正式用例：[cps-checksum](../../../testing/evidence/2026-09-07/TASK-20260907-004/cps-checksum/README.md)。归档所有实际源码/fixture/Java依赖、Wasm、HTTP原始帧、日志、预期实际、环境和复测入口；测试期间源文件hash不变。
- 本地8单测+1doctest、Java14向量、Host六codec与Finalize、真实HTTP规则修改重签均PASS；包/Host/Infrastructure严格Clippy、格式、Deno lint、架构与500行门禁PASS。
- 独立整体审查APPROVE，见证据 outputs/review.md；范围内无未解决P0/P1/P2。
- 修改文件：examples/protocol-packages/gmofg_cps 全包、src-tauri/crates/package-runtime/tests/gmofg_cps_component.rs、Infrastructure tests/phase10_http_pipeline/cps_checksum_wire.rs及父mod；任务文档与三个索引。没有Host产品源码、数据库Schema或前端变更，相关接口/ADR无需重写。
- 包README已记录完整导入方法、16路径、字段投影/签名算法、业务跳过/执行错误区别、数字语义与验证边界。父Finalize架构合同不变。
- 功能验收时CPS未提交、未推送、未触发CI，未安装到运行App；随后用户已授权本任务GitLab提交与MR交付，结果另行记录。用户已要求停止Windows监控，该远端构建不作为本任务功能完成条件。真实CPS/TLS/mTLS/终端业务与桌面UI均NOT_RUN。用户明确排除1e2、1E+2、-0数字词法输入，初审P1在缩小后范围外且未作修复，具体见需求变更及初审/复审报告。

## GitLab交付结果

- 2026-09-07 18:12:16 +08:00：本地提交f92e6674，独立交付分支feature/0907-004-cps-checksum上的提交bb0f795b，22文件，无无关DLL/CI变更。创建 [MR !4](http://172.16.3.60/other/intercept_proxy/-/merge_requests/4)，目标develop，opened、无冲突，单提交22文件；标题/描述与提交内容逐项一致。未实际合并。
- 交付证据：[gitlab-delivery](../../../testing/evidence/2026-09-07/TASK-20260907-004/gitlab-delivery/README.md)。源码/测试/说明/产物已推送，任务元数据与完整本地测试证据保持本地归档；提交使用[skip ci]及push ci.skip，不再监听Windows构建。
- 提交前只删除Java oracle prefix末尾多余空行，重新Java14向量PASS；功能算法、14条fixture与实际Wasm SHA未变。最终交付文件摘要和格式差异在delivery证据中单独保存，不改写原功能测试快照。

完成总结：按当前CPS源码实现独立可导入Wasm，完整下行编解码/展示/规则后重签已通过原Java与实际HTTP链验证，剩余未验证环境明确记录。
