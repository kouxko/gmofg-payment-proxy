# phase4-package-contract

- 任务：`TASK-20260829-002`
- 用例：`phase4-package-contract`
- 状态：`VERIFIED / APPROVED / CHECKPOINT READY`
- 执行时间：`2026-08-30 04:41:22 +08:00` 至 `2026-08-30 08:49:12 +08:00`
- 父用例：[phase3-recursive-document-contract](../phase3-recursive-document-contract/README.md)

## 目的与结果

建立唯一 `intercept-proxy-package-contract` crate，使 Rust 成为 API 1 Manifest、固定 JSON-RPC、FrameResult 与稳定错误 code wire 的唯一来源。crate 的唯一内部依赖是 Domain；Document、Schema、协议包身份和 ErrorCode 全部复用 Domain。未切换 WebSocket/Sidecar 生命周期，也未改变旧 protocol-scripting import 行为。

真实 TDD RED：五个 integration targets 已被 Cargo 自动发现，但新合同类型尚不存在，`cargo test -p intercept-proxy-package-contract --tests` 以 `E0432`、exit 101 失败。独立审查随后以 P1=3/P2=1 判定 `REQUEST CHANGES`；针对每项再次先得到真实 RED：Rust 缺少受验证 complete 构造/缓冲区校验，TS 缺少完整 RPC envelope guard 且接受 Domain-invalid identity/Schema，checker 的注释测试名、替代命名 owner、generated/MCP 漂移和宽泛 allowlist mutation 均未被拒绝。

修复后五个 targets 共 13/13 PASS，Phase4 checker 21/21、TS boundary guard 7/7、Domain 108/108；bindings fresh/deterministic、typecheck、lint、architecture、source-size、fmt、workspace strict Clippy 和 `git diff --check` PASS。前九门复验的前端为 63 files / 540 tests。资源 inventory 对活动 fixture/MCP snapshot 记录 SHA-256，checker 同时验证正式 evidence 副本的 SHA 和逐字节一致。

最终 delta review 仍发现 P1：MCP snapshot 只是摘要，攻击者可同步更新 snapshot/evidence/hash 夹带非合同 FrameResult；结构 owner 扫描可被 serde wire rename 绕过。新增精确 RED 后，MCP fixture 改为完整可执行 schema 与 canonical golden，逐项覆盖 Manifest、registration、八个 params/result、success/failure envelope、FrameResult 和完整 stable-code enum；checker 独立核对语义，即使 hash 被同步修改仍拒绝 `legacy_retry`。owner 扫描按 serialization/deserialization wire 字段集合识别，并覆盖 `rename`、`alias` 与 deserialize-only rename。

同轮 review 的两个 P2 也已显式关闭：TS decode success 直接调用 `isPackageDocument`，拒绝 unsafe integer 与包含非法递归值的 Document。新增共享 identity/SemVer corpus，由真实 `ProtocolPackageId::new`、`ProtocolPackageVersion::new` 和生成 metadata 分别执行并逐项比较；RED 实际发现 SemVer 正则与 Rust parser 在 core number 超过 `u64` 时不一致，现由 Rust 额外生成 core numeric max，TS/MCP 同步执行。`ErrorCode` 使用单一宏表生成 enum、serde wire、`ALL` 和 `as_str`，消除四份声明漂移。

最终复审继续发现一个 owner 扫描 P1 与权威示例差异。精确 RED 证明私有 serde struct、`#[serde(untagged)]` enum struct variant 和 `serde(flatten)` 可以绕过旧扫描；checker 现按平衡花括号解析私有/公开 struct 与 enum variant 的 wire 字段，并对合同 owner/Phase7 精确 symbol allowlist 外的 flatten fail-closed。任务权威 Manifest 示例 `com.example.payment` 保持不变；唯一 Domain `ProtocolPackageId` invariant 最小扩展为允许非空点分段，同时拒绝前导、尾随和重复点。Domain、contract crate、generated TS guard 与 MCP snapshot 共用同一 pattern/corpus，未增加第二 identity 类型或反向依赖。

最终 checker 复审又以三个正控证明上述扫描过宽：无关 filters flatten、注释/字符串中的 Phantom Manifest、以及未实现 Serde 的内部同字段 struct 都被误报。修复先词法屏蔽 Rust 注释与非 attribute 字符串，只分析 derive 或 manual impl 证明具备 Serialize/Deserialize eligibility 的类型；随后按方向处理 rename、alias、skip 与 flatten，递归解析本地 flattened type 的有效 wire 字段并用 visited set 截断循环。只有最终字段集合真实包含 `api/kind/package/document` 才报告 owner；旧 `ProtocolManifest` 本身不实现 Serde，已从 Phase7 Manifest owner allowlist 删除，不改变其 TOML 运行行为。

最后一个 manual Serde P1 证明，仅把 manual impl 当作 eligibility 仍会用声明字段误判或漏判实际 wire。新增 RED 覆盖声明为 `metadata/payload` 但 `serialize_field` 实际发出四个 Manifest key，以及 Deserialize impl 通过 `FIELDS` 和关联 Visitor match arms 接受四个 key；另以声明字段看似 Manifest、但 manual Serialize 只输出 harmless string 的正控防止回归。checker 现以无字符串花括号结构视图定位 impl，以保留字面量的同索引视图提取 `serialize_field/serialize_entry`、Deserialize fields const 和关联 Visitor 接受键；manual impl 的 owner 只由实际键集合决定，Rust lifetime 与字符字面量分别处理。

完整十门 checkpoint 的前九门及 Rust workspace 中除一个既有 MCP 非 loopback 环境用例外全部通过：前端 63 files / 538 tests；顶层 Rust 132/133 后仅 `production_bind_is_reachable_on_current_platform_interfaces_without_false_availability` 在当前 macOS 网络/VPN 环境等待非 loopback HTTP response 10 秒超时。定向复测同样超时，说明不是 Phase4 合同随机失败；本证据不把该全仓门禁记录为 PASS。

上述 132/133 与定向超时继续作为历史环境证据保留。用户随后授权使用 exact test binary `/Users/codin/Code/gmofg-payment-proxy/src-tauri/target/debug/deps/intercept_proxy-b171a3f7a5c9b203`（SHA-256 `c7dc870daca6f4f86eeebe29270ef65d4f61eab70b943b55cd994527544143aa`）并允许 firewall；第一次只用短函数名配合 `--exact` 实际发现 0 tests，明确标记 `NOT EVIDENCE`。改用完整模块测试名、`--all-features` 与 `--test-threads=1` 后目标用例 1/1 PASS。最终独立完整十门 checkpoint exit 0：前端 63 files / 541 tests、顶层 Rust 133/133、workspace all-target/all-feature exit 0；Phase4 checker 21/21、五个 Rust contract targets 13/13、TS 7/7、Domain 108/108、generated SHA-256 `897edb991e8bd7efc6d114ca4eb1c6b67eb162574e0bb764ebed7a93e39c3c9e`、七组 evidence byte copies 与 `git diff --check` 全部 PASS。独立 Verifier 结论 P0=0、P1=0、P2=0，G045 进入 `VERIFIED / APPROVED / CHECKPOINT READY`；TASK 总体仍为进行中。

## 合同覆盖

- Manifest 顶层只含 `api/kind/package/document`；HTTP direction 可空或含 Schema，Socket 双 direction 必须含 Schema；未知字段、错误 API 与非法 Schema definition 拒绝。
- `package.register` 是无 `id`、无响应的单向 notification；固定八个 Hook/Display 方法使用 string id 和严格 camelCase params。
- Document 是自然递归 JSON；Frame buffer 是 canonical padded Base64。
- FrameResult 是 `need_more/complete/reject` closed union；私有 `NonZeroUsize` newtype 令直接 Rust 构造无法表达 `consumedBytes=0`，cross/unknown fields 拒绝；合同提供 `validate_against_buffer_len` 给后续 adapter 使用，但本阶段不切换 runtime。
- JSON-RPC error data 直接暴露 Domain `ErrorCode`，不依赖 message 判断；完整 `ErrorCode::ALL` 由 Rust 生成给 TS unknown-boundary guard，未知稳定码拒绝。
- 单一 golden 覆盖 Manifest、register、八个 request、所有 method success 形态、stable-code error 和 FrameResult 全变体；Rust 全量 round-trip，TS 消费同一资源。TS 身份/Schema 校验参数由 Rust bindings 生成，不另建业务规则 owner。
- generated TS 与 Rust wire 同形；前端 guard 只校验 unknown boundary。MCP 完整 schema 与同字节 golden 以 SHA 和语义双重 fail-closed，并独立接受完整正例、拒绝跨合同 mutation。
- Phase7 legacy allowlist 精确到 `.rs` 文件和 symbol，拒绝宽泛、未使用及过期条目；Cargo `--list` 实际发现并核对五个非零 targets 的精确测试名，注释不能伪造执行。
- Manifest owner 扫描不依赖 `pub` 或类型名，覆盖 serde-renamed 私有 struct、untagged enum struct variant 与递归 flatten；注释、字符串、非 Serde 类型及最终不形成 Manifest 的 harmless flatten 不误报。

命令见 [replay/commands.txt](replay/commands.txt)，结构化结果见 [outputs/verification-summary.json](outputs/verification-summary.json)，实际 fixture 快照见 [resources/contract-fixtures.json](resources/contract-fixtures.json)。
