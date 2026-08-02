package com.interceptproxy.vpn

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ControlProtocolTest {
    @Test
    fun acceptsSupportedEnvelopeFields() {
        ControlProtocol.validateEnvelope(1, "request-1", "status")
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsUnknownProtocolVersion() {
        ControlProtocol.validateEnvelope(2, "request-2", "status")
    }

    @Test
    fun peerUidPolicyFailsClosedOutsideShellAndRoot() {
        assertTrue(ControlProtocol.isTrustedPeerUid(0))
        assertTrue(ControlProtocol.isTrustedPeerUid(2000))
        assertFalse(ControlProtocol.isTrustedPeerUid(10000))
        assertFalse(ControlProtocol.isTrustedPeerUid(-1))
    }
}
