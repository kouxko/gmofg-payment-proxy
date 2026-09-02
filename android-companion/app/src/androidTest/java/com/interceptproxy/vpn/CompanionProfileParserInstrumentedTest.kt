package com.interceptproxy.vpn

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

/** 依赖 Android 平台 org.json 的 Profile 与运行指纹解析测试。 */
class CompanionProfileParserInstrumentedTest {
    @Test
    fun pathMtuFaultDoesNotChangeAndroidTunMtu() {
        val profile = CompanionProfileParser.parse(profileJson())

        // 576 是 Rust 模拟的远端路径限制；TUN 保持 1280，超长包才能交给 Rust 处理。
        assertEquals(1_280, profile.mtu)
        assertEquals(1, profile.expectedProxyRouteCount)
    }

    @Test
    fun proxyRuntimeFactsAreRecomputedFromDecodedInput() {
        val profileJson = profileJson()
        val routeSource = JSONArray().put(routeSource())
        val runtime = ProxyRuntimeParser.parse(
            profileJson,
            runtimeJson(profileJson, routeSource),
        )

        assertEquals(
            ProxyRuntimeParser.sha256(ProxyRuntimeParser.canonicalJson(JSONObject(profileJson))),
            runtime.profileFingerprint,
        )
        assertEquals(
            ProxyRuntimeParser.sha256(
                ProxyRuntimeParser.canonicalJson(normalizedRoutes()),
            ),
            runtime.routeFingerprint,
        )
        assertEquals(1, runtime.routeCount)
    }

    @Test
    fun canonicalFingerprintMatchesRustForCidrAndUrl() {
        val value = JSONObject()
            .put(
                "destination_targets",
                JSONArray().put(
                    JSONObject()
                        .put("address", "10.0.0.0/8")
                        .put("ports", JSONArray().put(16_127)),
                ),
            )
            .put("server_url", "https://example.test:16127/path")

        val canonical = ProxyRuntimeParser.canonicalJson(value)

        assertEquals(
            "{\"destination_targets\":[{\"address\":\"10.0.0.0/8\",\"ports\":[16127]}]," +
                "\"server_url\":\"https://example.test:16127/path\"}",
            canonical,
        )
        assertEquals(
            "1b9889227509e4d1dca893ffc0d023e82e96258b84c34f03f4d550361a47db1a",
            ProxyRuntimeParser.sha256(canonical),
        )
    }

    @Test
    fun proxyRuntimeRejectsTornOrSelfReportedFacts() {
        val profileJson = profileJson()
        val routeSource = JSONArray().put(routeSource())
        val wrongCount = JSONObject(runtimeJson(profileJson, routeSource)).put("route_count", 0)
        assertThrows(IllegalArgumentException::class.java) {
            ProxyRuntimeParser.parse(profileJson, wrongCount.toString())
        }

        val falseFingerprint = JSONObject(runtimeJson(profileJson, routeSource))
            .put("route_fingerprint", "frontend-self-reported")
        assertThrows(IllegalArgumentException::class.java) {
            ProxyRuntimeParser.parse(profileJson, falseFingerprint.toString())
        }

        val wrongEndpoint = JSONObject(runtimeJson(profileJson, routeSource))
        wrongEndpoint.getJSONArray("routes").getJSONObject(0).put("proxy_port", 49_999)
        assertThrows(IllegalArgumentException::class.java) {
            ProxyRuntimeParser.parse(profileJson, wrongEndpoint.toString())
        }
    }

    private fun runtimeJson(profileJson: String, routeSource: JSONArray): String = JSONObject()
        .put("routes", normalizedRoutes())
        .put("route_source", routeSource)
        .put(
            "profile_fingerprint",
            ProxyRuntimeParser.sha256(ProxyRuntimeParser.canonicalJson(JSONObject(profileJson))),
        )
        .put(
            "route_fingerprint",
            ProxyRuntimeParser.sha256(ProxyRuntimeParser.canonicalJson(normalizedRoutes())),
        )
        .put("route_count", 1)
        .toString()

    private fun normalizedRoutes(): JSONArray = JSONArray().put(
        JSONObject()
            .put("listener_id", "listener-1")
            .put("original_destination", "example.test")
            .put("original_ports", JSONArray().put(443))
            .put("resolved_original_ips", JSONArray().put("203.0.113.10"))
            .put("proxy_host", "127.0.0.1")
            .put("proxy_port", 41_627),
    )

    private fun profileJson(): String = JSONObject()
        .put("id", "profile-1")
        .put(
            "target_applications",
            JSONArray().put(
                JSONObject()
                    .put("package_name", "example.target")
                    .put("uid", 12345),
            ),
        )
        .put("confirmed_shared_uids", JSONArray())
        .put("auto_resume_after_reboot", false)
        .put("proxy_routes", JSONArray().put(routeSource()))
        .put(
            "weak_network",
            JSONObject().put(
                "path_mtu",
                JSONObject()
                    .put("mtu", 576)
                    .put("mss_clamp", JSONObject.NULL)
                    .put("mode", "signal_too_big"),
            ),
        )
        .toString()

    private fun routeSource(): JSONObject = JSONObject()
        .put("listener_id", "listener-1")
        .put("original_destination", "example.test")
        .put("original_ports", JSONArray().put(443))
}
