package com.interceptproxy.vpn

import org.junit.After
import org.junit.Assert.assertEquals
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
    fun activeServiceStopTimeoutClosesTunAndRejectsLateCallback() {
        VpnRuntimeRegistry.startRequested("profile-1", runtime)
        val stop = VpnRuntimeRegistry.stopRequested()
        var tunCloseCount = 0

        assertTrue(
            VpnExternalStopCoordinator.completeActiveServiceStop(
                stopRequest = stop,
                message = "主线程超时，TUN 已强制关闭",
                releaseTun = { tunCloseCount += 1 },
            ),
        )
        assertEquals(1, tunCloseCount)
        assertEquals(stop.generation, VpnRuntimeRegistry.snapshot().generation)
        assertEquals("stopped", VpnRuntimeRegistry.snapshot().state)

        val newStart = VpnRuntimeRegistry.startRequested("profile-2", runtime)
        assertFalse(
            VpnExternalStopCoordinator.completeActiveServiceStop(
                stopRequest = stop,
                message = "late callback",
                releaseTun = { tunCloseCount += 1 },
            ),
        )
        assertEquals(1, tunCloseCount)
        assertTrue(VpnRuntimeRegistry.canStart(newStart))
    }

    @Test
    fun staleStartCannotPublishRunningAfterNewGeneration() {
        val staleGeneration = VpnRuntimeRegistry.startRequested("profile-1", runtime)
        VpnRuntimeRegistry.startRequested("profile-2", runtime)

        assertFalse(VpnRuntimeRegistry.running("profile-1", runtime, staleGeneration))
        assertFalse(VpnRuntimeRegistry.snapshot().verified)
    }
}
