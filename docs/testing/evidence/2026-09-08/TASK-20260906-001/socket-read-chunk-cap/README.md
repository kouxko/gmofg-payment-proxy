# Socket 单次读取 1 MiB 上限验收

TASK-20260906-001 / socket-read-chunk-cap / PASS

用户确认上限 1 MiB，默认保持 16 KiB；非法值报错且不截断。父用例：TASK-20260906-001 / socket-resource-limits / `docs/testing/evidence/2026-09-06/TASK-20260906-001/socket-resource-limits`。此次补齐父用例唯一未确认项，不覆盖原结果。

输入与结果见 [cases.json](inputs/cases.json)、[comparison.json](outputs/comparison.json)。1、16384、1048576 通过 JSON 往返和领域/候选预检；0、1048577、u32::MAX 保留输入并拒绝。候选在 Domain 返回 ListenerDomainInvalid，后续层 skipped；真实 Listener start/probe 在绑定/网络之前拒绝。1 MiB 合法配置实际 loopback：服务端收到 `00 73 6f 63 6b 65 74`，客户端收到 `72 65 70 6c 79 ff`，双向计数7/6，客户端半关闭后回复成立，stop后端口可再次绑定。字节结果由执行中的 assert_eq 和计数断言证明，是测试断言证据；未声称抓取 pcap。

执行顺序：先添加边界测试，候选RED证明越界被接受；领域最初fixture上游为空导致前置条件失败，修正为loopback有效地址后 RED只剩越界未拒绝（原始两个日志均保留）。实现共享MAX_SOCKET_READ_CHUNK_BYTES及两个校验入口后，6专项GREEN，受影响三crate全target/full-feature14组1111 PASS、0失败/忽略；workspace严格Clippy、fmt、bindings、架构、500行、typecheck全部PASS。

复测：在具备当前Deno/Rust依赖的仓库根目录运行 `sh docs/testing/evidence/2026-09-08/TASK-20260906-001/socket-read-chunk-cap/replay/run.sh`。所有端口在测试中从loopback动态分配，不需要外部服务或证书；用例自身stop并释放端口。环境版本见 [environment.json](outputs/environment.json)，命令和退出结果见 [static-results.json](outputs/static-results.json) 与 [rust-results.json](outputs/rust-results.json)。定向日志为 [green.log](outputs/green.log)，完整输出为 [affected-rust.log](outputs/affected-rust.log)。

资源：`resources/source-inputs.tar.gz` 保存当次使用的1181个tracked源码及fixture；来源列表在 source-inventory.json。复测历史状态时在独立工作副本提取该快照，不能覆盖他人工作目录。`resources/requirements.md` 保存当前需求条款；它与 docs/requirements.md 直接逐字节比较，且当前文档已过架构检查。测试期间1181份输入无变化，见 [source-stability.json](outputs/source-stability.json)。归档均为实际输入，不把未跟踪CPS资源纳入范围。

N/A：无UI生产改动（调用方typecheck已做）；透明raw Socket不涉及Frame/Decode/Rules/Encode；无TLS证书、真实设备、外部业务或远端CI。本次不验证Windows/Android内存总量或长稳。独立最终审查quick_build_review：APPROVE，P0/P1/P2=0；源码、测试计数、资源快照、元数据、复测和链接readback通过。
