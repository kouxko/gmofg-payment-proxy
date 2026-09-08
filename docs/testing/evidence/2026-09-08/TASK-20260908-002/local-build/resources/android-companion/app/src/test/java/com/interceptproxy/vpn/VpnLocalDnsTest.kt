package com.interceptproxy.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class VpnLocalDnsTest {
    @Test
    fun usesBenchmarkRangeAddressHandledInsideTun() {
        assertEquals("198.18.0.1", VpnLocalDns.ADDRESS)
    }
}
