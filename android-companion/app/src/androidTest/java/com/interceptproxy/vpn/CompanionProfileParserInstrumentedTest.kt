package com.interceptproxy.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * 这里必须在 Android 运行时验证，因为 [CompanionProfileParser] 使用平台的 org.json。
 * 本地 JVM 提供的 android.jar 只是会抛异常的桩实现，无法证明真实解析行为。
 */
class CompanionProfileParserInstrumentedTest {
    @Test
    fun pathMtuFaultDoesNotChangeAndroidTunMtu() {
        val profile = CompanionProfileParser.parse(
            """
            {
              "target_applications": [
                {
                  "package_name": "example.target",
                  "signing_sha256": "AA",
                  "uid": 12345
                }
              ],
              "confirmed_shared_uids": [],
              "auto_resume_after_reboot": false,
              "weak_network": {
                "path_mtu": {
                  "mtu": 576,
                  "mss_clamp": null,
                  "mode": "signal_too_big"
                }
              }
            }
            """.trimIndent(),
        )

        // 576 是 Rust 模拟的远端路径限制；TUN 保持 1280，超长包才能交给 Rust 处理。
        assertEquals(1_280, profile.mtu)
    }
}
