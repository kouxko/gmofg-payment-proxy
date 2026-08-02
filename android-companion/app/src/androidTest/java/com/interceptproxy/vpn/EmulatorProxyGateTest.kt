package com.interceptproxy.vpn

import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.FixMethodOrder
import org.junit.Test
import org.junit.runners.MethodSorters
import java.net.InetSocketAddress
import java.net.Socket
import java.nio.charset.Charset
import kotlin.system.measureTimeMillis

/**
 * Emulator-only proxy rule matrix.
 *
 * Every request traverses adb reverse and the production Reverse Listener/pipeline. The upstream
 * is a local test fixture, so a D48 result here is regression evidence only. It is not real
 * A920MAX or GMO-FG acceptance evidence.
 */
@FixMethodOrder(MethodSorters.NAME_ASCENDING)
class EmulatorProxyGateTest {
    private val shiftJis: Charset = Charset.forName("Shift_JIS")

    @Before
    fun requireDedicatedProxyFixture() {
        val enabled = InstrumentationRegistry.getArguments()
            .getString("interceptProxyGateEnabled")
            .toBoolean()
        assumeTrue(
            "仅由 test-support/emulator-proxy-gate/run.sh 启动专用代理夹具后执行",
            enabled,
        )
    }

    @Test
    fun aBaselinePreservesShiftJisD48BytesAndRawHeaders() {
        val response = exchange("/matrix/baseline", "DLL-DEVICE-INFO".toByteArray())
        val parsed = requireNotNull(parseResponse(response))
        val expectedText = "{\"result\":\"D48\",\"message\":\"端末情報更新が必要です\"}"
        val expectedBytes = expectedText.toByteArray(shiftJis)

        assertEquals(200, parsed.status)
        assertTrue(parsed.head.contains("\r\nX-Upstream-Fixture: simulated-d48\r\n"))
        assertTrue(parsed.head.contains("\r\nX-Header-Order: first\r\nX-Header-Order: second\r\n"))
        assertArrayEquals(expectedBytes, parsed.body)
    }

    @Test
    fun bRequestRuleForwardsModifiedHeaderAndJsonBody() {
        val response = exchange(
            "/matrix/request-modify",
            "{\"amount\":100,\"operation\":\"probe\"}".toByteArray(),
            contentType = "application/json; charset=UTF-8",
        )

        assertEquals(200, requireNotNull(parseResponse(response)).status)
    }

    @Test
    fun cResponseRuleChangesStatusHeaderAndShiftJisBody() {
        val response = requireNotNull(parseResponse(exchange("/matrix/response-modify")))
        val expected = "{\"result\":\"R48\",\"message\":\"代理修改\"}".toByteArray(shiftJis)

        assertEquals(503, response.status)
        assertTrue(response.head.contains("\r\nX-Response-Rule: applied\r\n", ignoreCase = true))
        assertArrayEquals(expected, response.body)
    }

    @Test
    fun dMockRuleReturnsWithoutContactingUpstream() {
        val response = requireNotNull(parseResponse(exchange("/matrix/mock")))

        assertEquals(202, response.status)
        assertTrue(response.head.contains("\r\nX-Mock-Rule: applied\r\n", ignoreCase = true))
        assertArrayEquals("{\"result\":\"MOCK\"}".toByteArray(), response.body)
    }

    @Test
    fun eNthHitOneShotRuleAppliesOnlyToSecondRequest() {
        val first = requireNotNull(parseResponse(exchange("/matrix/nth-hit")))
        val second = requireNotNull(parseResponse(exchange("/matrix/nth-hit")))
        val third = requireNotNull(parseResponse(exchange("/matrix/nth-hit")))

        assertTrue(!first.head.contains("X-Nth-Hit", ignoreCase = true))
        assertTrue(second.head.contains("\r\nX-Nth-Hit: second-only\r\n", ignoreCase = true))
        assertTrue(!third.head.contains("X-Nth-Hit", ignoreCase = true))
    }

    @Test
    fun fDelayRuleAddsObservableLatency() {
        lateinit var response: ByteArray
        val elapsed = measureTimeMillis { response = exchange("/matrix/delay") }

        assertEquals(200, requireNotNull(parseResponse(response)).status)
        assertTrue("expected at least 180 ms, actual ${elapsed} ms", elapsed >= 180)
    }

    @Test
    fun gTruncateRuleClosesBodyBeforeDeclaredContentLength() {
        val response = requireNotNull(parseResponse(exchange("/matrix/truncate")))
        val declaredLength = Regex("(?im)^Content-Length:\\s*(\\d+)\\s*$")
            .find(response.head)
            ?.groupValues
            ?.get(1)
            ?.toInt()

        assertNotNull(declaredLength)
        assertEquals(7, response.body.size)
        assertTrue(requireNotNull(declaredLength) > response.body.size)
    }

    @Test
    fun hDropResponseRuleReturnsNoHttpResponse() {
        assertNull(parseResponse(exchange("/matrix/drop")))
    }

    @Test
    fun iDisconnectBeforeUpstreamReturnsNoHttpResponse() {
        assertNull(parseResponse(exchange("/matrix/disconnect")))
    }

    @Test
    fun jTransactionUsesIndependentListenerAndPreservesShiftJisD48() {
        val response = requireNotNull(
            parseResponse(
                exchange(
                    path = "/transaction/authorize",
                    body = "TRANSACTION-REQUEST".toByteArray(shiftJis),
                    port = 6556,
                ),
            ),
        )
        val expected = "{\"result\":\"D48\",\"message\":\"端末情報更新が必要です\"}"
            .toByteArray(shiftJis)

        assertEquals(200, response.status)
        assertTrue(response.head.contains("\r\nX-Upstream-Channel: transaction\r\n"))
        assertArrayEquals(expected, response.body)
    }

    private fun exchange(
        path: String,
        body: ByteArray = ByteArray(0),
        contentType: String = "application/octet-stream",
        port: Int = 6555,
    ): ByteArray = Socket().use { socket ->
        socket.connect(InetSocketAddress("127.0.0.1", port), 10_000)
        socket.soTimeout = 10_000
        val output = socket.getOutputStream()
        val head = buildString {
            append("POST $path HTTP/1.1\r\n")
            append("Host: 127.0.0.1:$port\r\n")
            append("Content-Type: $contentType\r\n")
            append("Content-Length: ${body.size}\r\n")
            append("Connection: close\r\n\r\n")
        }.toByteArray()
        output.write(head)
        output.write(body)
        output.flush()
        socket.getInputStream().readBytes()
    }

    private fun parseResponse(response: ByteArray): ParsedResponse? {
        val separator = response.indices.firstOrNull { index ->
            index + 3 < response.size &&
                response[index] == '\r'.code.toByte() &&
                response[index + 1] == '\n'.code.toByte() &&
                response[index + 2] == '\r'.code.toByte() &&
                response[index + 3] == '\n'.code.toByte()
        } ?: return null
        val head = String(response, 0, separator, Charsets.ISO_8859_1)
        val status = head.lineSequence().first().split(' ')[1].toInt()
        return ParsedResponse(
            status = status,
            head = head,
            body = response.copyOfRange(separator + 4, response.size),
        )
    }

    private data class ParsedResponse(
        val status: Int,
        val head: String,
        val body: ByteArray,
    )
}
