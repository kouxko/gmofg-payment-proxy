# A920 物理设备受控恢复验收

TASK-20260905-001 / a920-controlled-recovery / PASS

用户允许A920安装测试APK、启停VPN和短暂断网。设备2740072778 / A920MAX / API29 / armeabi-v7a，VPN已授权allow；所有测试只触及受控target/isolation probe与本机loopback fixture，不发送GMO真实交易，不替代桌面UI/LAN端点协调端到端验收。

## 结果与范围

- SavedRecoveryAdmissionInstrumentedTest 真机2/2 PASS：保护开启的保存恢复等待、不启动Service；关闭保护保持已有设备恢复行为。见 outputs/admission-instrumentation.txt。
- 权威真实执行为 [attempt3 report](outputs/controlled-v2-attempt3/report.json)：22个v2请求完整Frame，目标App在两轮各访问203.0.113.48:16127/16627，四次实际请求/响应逐字节匹配；非目标UID访问原目标明确连接超时，fixture未收到请求。
- 两轮running均新增tun0，active profile id/fingerprint、route fingerprint和count=2精确匹配；停止1秒间隔heartbeat后等待5.75秒，设备变stopped且TUN消失；旧epoch的apply_active精确返回ANDROID_CONTROL_LEASE_STALE，后续status仍stopped；显式apply新UUID epoch才恢复running并再次成功收发，最后stop清理。设备status不暴露generation/epoch（unsupported serial也保留原样）；新epoch由两个实际控制请求证明，不伪造设备字段。
- 四个wire comparison均request_exact=true/response_exact=true，全部22对控制Frame长度/JSON/hex一致；88份Android/Engine/协议源码测试期间逐字节稳定，见 outputs/evidence-consistency.json。请求body精确为 `{"operation":"probe"}`，路径区分phase/channel；返回是受控JSON D48，所有完整bytes与TCP chunks保存在 outputs/controlled-v2-attempt3/fixtures 和 probes。
- cleanup PASS：原APK完整bytes恢复；原SharedPreferences4项经同签名instrumentation强类型commit及读回验证恢复；新装两个probe与test package删除；自建端口转发/reverse、TUN、VpnService清零。原设备备份文件在归档后移除，原VPN授权allow未改变。见 outputs/cleanup-comparison.json。

## 真实前置资源

resources/device-original-companion.apk为测试前实际pull的原APK；resources/original-runtime-preferences.json为原4项强类型配置备份，必须保留，禁止手改。resources/current-companion.apk为此次受测release，逐字节等于父用例 `docs/testing/evidence/2026-09-05/TASK-20260905-001/manual-recovery/android-atomic-hot-apply/resources/apk/android-companion-release.apk`；来源和源文件清单见 outputs/current-apk-provenance.json，当前源文件快照为resources/current-android-sources.tar.gz。target-probe-debug.apk与isolation-probe-debug.apk来自仓库原fixture构建输出，两package在本轮前不存在。

resources/stable-androidTest-v2.apk包含备份helper和现有admission测试，与当前/原Companion签名相同（SDK apksigner实测输出归档）。测试APK只用于instrumentation，不卸载目标包/数据；helper源为 replay/RuntimePreferencesBackupInstrumentedTest-v2.kt。v1 APK/source/失败日志保留，不供当前复测；v1的testContext外部目录在此设备不可用，v2只改为targetContext的唯一备份位置。备份使用createNewFile，已存在即失败，不覆盖；恢复先严格校验package/name/schema/type，再commit并逐项读回。默认备份 `/storage/emulated/0/Android/data/com.interceptproxy.vpn/files/intercept_proxy_vpn_runtime.backup.json`。

## 从零复测

必须在新的证据目录执行，不能覆盖本次档案。ADB连接同等可用测试设备；确认无活动VPN/TUN、预留reverse6555/6556未占用，target/isolation/test packages无现有数据冲突。先pull原Companion APK，记录服务/接口/端口/权限与package状态。若不同设备，替换serial并重新采集原APK/配置，不能用本档案的原用户配置覆盖其他设备。

1. 安装 `stable-androidTest-v2.apk`；执行 `adb -s SERIAL shell am instrument -w -r -e class com.interceptproxy.vpn.RuntimePreferencesBackupInstrumentedTest#backupOrRestoreRuntimePreferences -e action backup com.interceptproxy.vpn.test/androidx.test.runner.AndroidJUnitRunner`。必须看到OK(1 test)、backup_complete；按backup_path pull原始JSON并解析。备份已有文件时停止，不删除未知备份。
2. 在新输出中生成recovery-backup-gate.json：package=com.interceptproxy.vpn、serial精确、backup_complete=true、restore_supported=true、backup_file指向刚获取的非空原JSON。只有真实备份成功才填写true。当前已执行门禁实例见outputs/recovery-backup-gate.json。
3. `adb -s SERIAL install -r current-companion.apk`，安装两个probe APK；运行 `SavedRecoveryAdmissionInstrumentedTest`并要求OK(2 tests)。这两项只证明准入，不替代下步TUN。
4. 运行 `python3 replay/controlled-v2-v3.py --serial SERIAL --output NEW_OUTPUT --recovery-backup NEW_GATE_JSON`。该入口动态建两个host loopback fixture，经ADB reverse映射device6555/6556；只创建/删除自己的forward/reverse，start前验证stopped，失败也明确stop并检查TUN退出。物理请求、响应、控制帧、ADB调用参数、每步时间以及失败均持久保存。
5. 无论成功/失败，先确认测试VPN停止/映射清理，再用同一helper执行action restore，必须restore_complete与readback_verified=true。重新安装当次原APK后再次验证原配置读回；本次实际restore与原APKbyte readback命令输出见outputs/recovery-restore-original-code-instrumentation.txt、restored-apk-pull.txt。仅删除本轮创建的备份文件和新安装测试包，不卸载原Companion。
6. 检查最终服务无、TUN无、own映射无、原APK/config相等、原授权保持。未通过cleanup不能报告整体PASS。

如需重建辅助APK：在有仓库固定stableCompanion签名配置/Gradle/Android SDK的环境，从仓库根运行 `sh replay/build-backup-helper.sh`（传入本case真实相对路径）。它创建独立临时sourceSet，仅构建androidTest并排除无关四ABI Rust构建；不操作设备。直接复测可使用归档已签名APK，无需重建。

## 保留的失败与限制

attempt1的`am start -W`等待自动finish Activity导致30秒超时，实际进程已启动；按生产wake用法移除-W，仍以v2status和probe文件校验结果，不把命令返回当成功。attempt2发现本设备exec-out会把cat错误合并到stdout且返回0；切到无PTY的`adb shell -T`后使用真实退出码/独立stderr，文件尚未生成时继续有界轮询，任意错误文本不再被当作响应。两次失败及各自脚本完整保留，未改产品来绕过失败；attempt3最终PASS。

N/A：真实GMO服务/支付、桌面UI触发、LAN热协调实机、业务协议Decode/Rules/Encode、Windows、远端CI与Release。原父任务这些独立本地合同证据继续保留；本用例只补真实Device LocalSocket/TUN/JNI、失联停机及手动恢复门禁。独立最终审查frontend_followup_review：APPROVE，P0/P1/P2=0，确认父任务设备门禁闭合，可完成日期归档。
