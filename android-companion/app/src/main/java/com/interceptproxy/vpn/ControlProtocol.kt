package com.interceptproxy.vpn

import org.json.JSONObject

/** 桌面与 Companion 共用的 v2 长度前缀 JSON 信封。 */
object ControlProtocol {
    const val VERSION = 2
    const val MAX_FRAME_BYTES = 1024 * 1024
    const val SOCKET_NAME = "intercept_proxy_vpn"

    data class Request(
        val requestId: String,
        val operation: String,
        val payload: JSONObject,
    )

    fun parseRequest(bytes: ByteArray): Request {
        require(bytes.isNotEmpty() && bytes.size <= MAX_FRAME_BYTES) { "控制帧长度无效" }
        val root = JSONObject(bytes.toString(Charsets.UTF_8))
        val requestId = root.getString("request_id")
        val operation = root.getString("operation")
        validateEnvelope(root.getInt("version"), requestId, operation)
        return Request(
            requestId = requestId,
            operation = operation,
            payload = root.optJSONObject("payload") ?: JSONObject(),
        )
    }

    /**
     * 对信封中不依赖 Android 框架的字段执行 fail-closed 校验。
     *
     * 这个函数刻意只接收 Kotlin 基础类型，使协议边界能够在普通 JVM 单元测试中覆盖；
     * Android SDK 自带的 `org.json` 在本地单元测试环境中只是会抛异常的 Stub，JSON 文本
     * 解码本身则继续由设备运行时的 [JSONObject] 完成。
     */
    internal fun validateEnvelope(version: Int, requestId: String, operation: String) {
        require(version == VERSION) { "控制协议版本不受支持" }
        require(requestId.isNotBlank() && requestId.length <= 128) { "request_id 无效" }
        require(operation in SUPPORTED_OPERATIONS) { "控制 operation 不受支持：$operation" }
    }

    fun success(requestId: String, status: JSONObject): ByteArray =
        response(requestId, true, status, null, null)

    fun failure(requestId: String, code: String, message: String): ByteArray =
        response(requestId, false, null, code, message)

    fun isTrustedPeerUid(uid: Int): Boolean = uid == ROOT_UID || uid == SHELL_UID

    private fun response(
        requestId: String,
        ok: Boolean,
        status: JSONObject?,
        errorCode: String?,
        errorMessage: String?,
    ): ByteArray = JSONObject()
        .put("version", VERSION)
        .put("request_id", requestId)
        .put("ok", ok)
        .put("status", status ?: JSONObject.NULL)
        .put("error_code", errorCode ?: JSONObject.NULL)
        .put("error_message", errorMessage ?: JSONObject.NULL)
        .toString()
        .toByteArray(Charsets.UTF_8)

    private val SUPPORTED_OPERATIONS = setOf(
        "start",
        "apply",
        "stop",
        "emergency_restore",
        "status",
        "heartbeat",
    )
    private const val ROOT_UID = 0
    private const val SHELL_UID = 2000
}
