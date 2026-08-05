package com.interceptproxy.vpn

import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VpnRuntimeRegistryTest {
    private val runtime = ProxyRuntimeFacts("profile", "routes", 1)

    @After
    fun reset() = VpnRuntimeRegistry.resetForTest()

    @Test
    fun immediateStopInvalidatesQueuedStartUntilTeardownIsConfirmed() {
        val startGeneration = VpnRuntimeRegistry.startRequested("profile-1", runtime)
        val stop = VpnRuntimeRegistry.stopRequested()

        assertFalse(VpnRuntimeRegistry.canStart(startGeneration))
        assertTrue(stop.requiresTeardownConfirmation)
        assertFalse(VpnRuntimeRegistry.snapshot().verified)

        assertTrue(VpnRuntimeRegistry.confirmStopped(stop.generation))
        assertTrue(VpnRuntimeRegistry.snapshot().verified)
    }

    @Test
    fun nullServiceStopCannotClaimVerifiedStoppedBeforeTeardown() {
        VpnRuntimeRegistry.startRequested("profile-1", runtime)
        VpnRuntimeRegistry.stopRequested()

        val pending = VpnRuntimeRegistry.snapshot()
        assertTrue(pending.state == "stop_requested")
        assertFalse(pending.verified)
    }

    @Test
    fun queuedStartCancelledBeforeServiceCreationSettlesAsVerifiedStopped() {
        val startGeneration = VpnRuntimeRegistry.startRequested("profile-1", runtime)
        val stop = VpnRuntimeRegistry.stopRequested()
        var nativeStopped = false
        var queuedServiceStopped = false

        VpnExternalStopCoordinator.completeWithoutActiveService(
            stopRequest = stop,
            message = "测试停止完成",
            stopNativeDataPlane = { nativeStopped = true },
            stopQueuedService = { queuedServiceStopped = true },
        )

        assertTrue(nativeStopped)
        assertTrue(queuedServiceStopped)
        assertFalse(VpnRuntimeRegistry.canStart(startGeneration))
        assertTrue(VpnRuntimeRegistry.snapshot().verified)
        assertTrue(VpnRuntimeRegistry.snapshot().state == "stopped")
    }

    @Test
    fun staleStartCannotPublishRunningAfterNewGeneration() {
        val staleGeneration = VpnRuntimeRegistry.startRequested("profile-1", runtime)
        VpnRuntimeRegistry.startRequested("profile-2", runtime)

        assertFalse(VpnRuntimeRegistry.running("profile-1", runtime, staleGeneration))
        assertFalse(VpnRuntimeRegistry.snapshot().verified)
    }
}
