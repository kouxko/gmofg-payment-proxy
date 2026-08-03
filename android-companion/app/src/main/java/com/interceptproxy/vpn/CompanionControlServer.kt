package com.interceptproxy.vpn

import android.content.Context
import android.net.LocalServerSocket
import android.net.LocalSocket
import android.net.VpnService
import android.util.Log
import java.io.DataInputStream
import java.io.DataOutputStream
import java.util.concurrent.Executors
import org.json.JSONArray
import org.json.JSONObject

/**
 * `adb forward tcp:0 localabstract:intercept_proxy_vpn` 的设备端服务。
 *
 * Android 的 LocalSocket 提供对端 Linux credentials。只有 uid=2000(shell) 或 uid=0(root)
 * 可以发送命令；读取 credentials 失败时直接关闭连接，绝不回退到“只要能连就信任”。
 */
class CompanionControlServer(private val context: Context) {
    private val acceptExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "intercept-control-accept").apply { isDaemon = true }
    }

    fun start() {
        acceptExecutor.execute {
            runCatching {
                // LocalServerSocket 在 API 28 才声明实现 Closeable，不能在 minSdk 26 上使用
                // Kotlin `use`。`close()` 本身从 API 1 就存在，因此显式 finally 最安全。
                val server = LocalServerSocket(ControlProtocol.SOCKET_NAME)
                try {
                    while (!Thread.currentThread().isInterrupted) {
                        val socket = server.accept()
                        // 单个 adb 客户端可能在读取响应前主动断开，写回时会出现
                        // Broken pipe。连接级 I/O 失败只能结束当前请求，不能让唯一的
                        // accept 线程和后续所有桌面控制命令一起退出。
                        runCatching { handleConnection(socket) }
                            .onFailure { error -> Log.w(TAG, "控制连接异常关闭，继续接受下一连接", error) }
                    }
                } finally {
                    server.close()
                }
            }.onFailure { error -> Log.e(TAG, "Companion 控制 socket 已停止", error) }
        }
    }

    private fun handleConnection(socket: LocalSocket) {
        socket.use { connection ->
            connection.soTimeout = IO_TIMEOUT_MILLIS
            val peerUid = runCatching { connection.peerCredentials.uid }.getOrElse { error ->
                Log.e(TAG, "无法读取控制连接 peer credentials，已拒绝", error)
                return
            }
            if (!ControlProtocol.isTrustedPeerUid(peerUid)) {
                Log.e(TAG, "拒绝非 shell/root 控制连接 uid=$peerUid")
                return
            }

            val input = DataInputStream(connection.inputStream)
            val length = runCatching { input.readInt() }.getOrElse { return }
            if (length <= 0 || length > ControlProtocol.MAX_FRAME_BYTES) {
                Log.e(TAG, "拒绝无效控制帧长度：$length")
                return
            }
            val payload = ByteArray(length)
            runCatching { input.readFully(payload) }.getOrElse { return }

            val response = runCatching {
                val request = ControlProtocol.parseRequest(payload)
                dispatch(request)
            }.getOrElse { error ->
                val requestId = runCatching {
                    JSONObject(payload.toString(Charsets.UTF_8)).optString("request_id", "invalid")
                }.getOrDefault("invalid")
                ControlProtocol.failure(
                    requestId,
                    "ANDROID_PROTOCOL_REQUEST_INVALID",
                    error.message ?: "控制请求无效",
                )
            }
            DataOutputStream(connection.outputStream).use { output ->
                output.writeInt(response.size)
                output.write(response)
                output.flush()
            }
        }
    }

    private fun dispatch(request: ControlProtocol.Request): ByteArray = when (request.operation) {
        "start", "apply" -> startOrApply(request)
        "stop" -> {
            VpnRuntimeRegistry.stopRequested()
            context.startService(InterceptVpnService.stopIntent(context))
            ControlProtocol.success(request.requestId, statusJson())
        }
        "emergency_restore" -> {
            RuntimeStateStore(context).autoResumeEnabled = false
            VpnRuntimeRegistry.stopRequested()
            context.stopService(android.content.Intent(context, InterceptVpnService::class.java))
            VpnRuntimeRegistry.stopped("已执行紧急恢复并关闭自动重启。")
            ControlProtocol.success(request.requestId, statusJson())
        }
        "status" -> ControlProtocol.success(request.requestId, statusJson())
        else -> ControlProtocol.failure(
            request.requestId,
            "ANDROID_PROTOCOL_OPERATION_INVALID",
            "控制操作不受支持。",
        )
    }

    private fun startOrApply(request: ControlProtocol.Request): ByteArray {
        val profileJson = request.payload.optJSONObject("profile")?.toString()
            ?: return ControlProtocol.failure(
                request.requestId,
                "ANDROID_PROFILE_MISSING",
                "start/apply 缺少 profile。",
            )
        val proxyRuntimeJson = request.payload.optJSONObject("proxy_runtime")
            ?.toString()
            ?: "{\"routes\":[]}"
        val profile = runCatching { CompanionProfileParser.parse(profileJson) }.getOrElse { error ->
            return ControlProtocol.failure(
                request.requestId,
                "ANDROID_PROFILE_INVALID",
                "Profile JSON 无效：${error.message}",
            )
        }
        val inventory = PackageInventory.collect(context.packageManager)
        // start/apply 与 Service 启动共用 Rust 的唯一业务校验实现。Kotlin 不判断
        // 包签名、UID、shared UID 或弱网参数，只采集系统安装清单并管理生命周期。
        val rustError = NativeBridge.validateProfile(profile.rawJson, inventory.toInventoryJson())
        if (rustError.isNotEmpty()) {
            return ControlProtocol.failure(request.requestId, "ANDROID_PROFILE_INVALID", rustError)
        }
        if (VpnService.prepare(context) != null) {
            return ControlProtocol.failure(
                request.requestId,
                "ANDROID_VPN_CONSENT_REQUIRED",
                "Android 系统 VPN 授权尚未完成，请先打开授权页。",
            )
        }

        RuntimeStateStore(context).profileJson = profileJson
        VpnRuntimeRegistry.startRequested(JSONObject(profileJson).getString("id"))
        return runCatching {
            context.startForegroundService(
                InterceptVpnService.startIntent(context, profileJson, proxyRuntimeJson),
            )
            ControlProtocol.success(request.requestId, statusJson())
        }.getOrElse { error ->
            VpnRuntimeRegistry.faulted("启动 VpnService 失败：${error.message}")
            ControlProtocol.failure(
                request.requestId,
                "ANDROID_VPN_START_FAILED",
                error.message ?: "启动 VpnService 失败",
            )
        }
    }

    private fun statusJson(): JSONObject {
        val snapshot = VpnRuntimeRegistry.snapshot()
        return JSONObject()
            // Android 公共 API 无法可靠读取 adb 选择的设备 serial；桌面已有该上下文。
            .put("serial", "")
            .put("state", snapshot.state)
            .put("verified", true)
            .put("transport", "local_abstract_socket")
            .put("active_profile_id", snapshot.activeProfileId ?: JSONObject.NULL)
            .put("companion_process_running", true)
            .put("message", snapshot.message)
            .put("unsupported_fields", JSONArray().put("serial"))
            .put("stats", runCatching { JSONObject(NativeBridge.statsJson()) }.getOrDefault(JSONObject()))
    }

    companion object {
        private const val TAG = "InterceptControl"
        private const val IO_TIMEOUT_MILLIS = 5_000
    }
}
