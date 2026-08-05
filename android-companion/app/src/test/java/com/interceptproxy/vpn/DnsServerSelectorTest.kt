package com.interceptproxy.vpn

import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Test

class DnsServerSelectorTest {
    @Test
    fun preservesPhysicalNetworkDnsOrderAndRemovesDuplicates() {
        val ipv4 = InetAddress.getByName("192.0.2.53")
        val ipv6 = InetAddress.getByName("2001:db8::53")

        assertEquals(
            listOf(ipv4, ipv6),
            DnsServerSelector.select(
                isPhysicalNetwork = true,
                candidates = listOf(ipv4, ipv6, ipv4),
            ),
        )
    }

    @Test(expected = IllegalStateException::class)
    fun rejectsDnsFromVpnNetwork() {
        DnsServerSelector.select(
            isPhysicalNetwork = false,
            candidates = listOf(InetAddress.getByName("192.0.2.53")),
        )
    }

    @Test(expected = IllegalStateException::class)
    fun emptyPhysicalDnsFailsInsteadOfInjectingThirdPartyFallback() {
        DnsServerSelector.select(isPhysicalNetwork = true, candidates = emptyList())
    }
}
