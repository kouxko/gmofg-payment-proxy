package com.interceptproxy.vpn

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ControlLeaseRecoveryTest {
    private val runtime = ProxyRuntimeFacts("profile", "routes", 0)

    @After
    fun resetRegistry() {
        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun protectedRecoveryWaitsWithoutEnqueueGenerationOrLeaseMutation() {
        val coordinator = ControlLeaseCoordinator(timeoutMillis = 5_000)
        val oldGeneration = VpnRuntimeRegistry.startRequested("profile-old", runtime)
        assertTrue(VpnRuntimeRegistry.running("profile-old", runtime, oldGeneration))
        coordinator.configure(oldGeneration, "epoch-old", enabled = true, nowMillis = 0)
        val before = VpnRuntimeRegistry.snapshot()
        var enqueueCount = 0

        val result = coordinator.recover(
            profileId = "profile-saved",
            runtime = runtime,
            stopVpnOnControlLoss = true,
        ) { enqueueCount += 1 }

        assertEquals(RecoveryAdmission.WaitingForDesktopControl, result.getOrThrow())
        assertEquals(0, enqueueCount)
        assertEquals(before, VpnRuntimeRegistry.snapshot())
        assertTrue(coordinator.heartbeat("epoch-old", nowMillis = 4_999))
        assertNull(coordinator.claimExpiredStop(nowMillis = 4_999))
    }

    @Test
    fun protectedWaitThenManualStartKeepsHeartbeatAndFiveSecondBoundary() {
        val coordinator = ControlLeaseCoordinator(timeoutMillis = 5_000)

        assertEquals(
            RecoveryAdmission.WaitingForDesktopControl,
            coordinator.recover(
                profileId = "profile-saved",
                runtime = runtime,
                stopVpnOnControlLoss = true,
            ) { error("protected recovery must not enqueue") }.getOrThrow(),
        )
        val manual = coordinator.start(
            profileId = "profile-saved",
            runtime = runtime,
            ownerEpoch = "desktop-epoch",
            enabled = true,
            nowMillis = { 100 },
        ) {}

        assertTrue(manual.isSuccess)
        assertTrue(coordinator.heartbeat("desktop-epoch", nowMillis = 100))
        assertNull(coordinator.claimExpiredStop(nowMillis = 5_099))
        assertEquals(
            manual.getOrThrow(),
            coordinator.claimExpiredStop(nowMillis = 5_100)?.generation?.minus(1),
        )
    }

    @Test
    fun unprotectedDeviceOnlyRecoveryEnqueuesWithoutLeaseExpiry() {
        val coordinator = ControlLeaseCoordinator(timeoutMillis = 5_000)
        var enqueuedGeneration: Long? = null

        val result = coordinator.recover(
            profileId = "profile-device-only",
            runtime = runtime,
            stopVpnOnControlLoss = false,
        ) { generation -> enqueuedGeneration = generation }

        val started = result.getOrThrow() as RecoveryAdmission.Started
        assertEquals(started.generation, enqueuedGeneration)
        assertTrue(VpnRuntimeRegistry.canStart(started.generation))
        assertFalse(coordinator.heartbeat("unmanaged", nowMillis = 1_000))
        assertNull(coordinator.claimExpiredStop(nowMillis = 50_000))
    }

    @Test
    fun unprotectedRecoveryEnqueueFailureRestoresPreviousGenerationAndLease() {
        val coordinator = ControlLeaseCoordinator(timeoutMillis = 5_000)
        val oldGeneration = VpnRuntimeRegistry.startRequested("profile-old", runtime)
        assertTrue(VpnRuntimeRegistry.running("profile-old", runtime, oldGeneration))
        coordinator.configure(oldGeneration, "epoch-old", enabled = true, nowMillis = 0)

        val result = coordinator.recover(
            profileId = "profile-device-only",
            runtime = runtime,
            stopVpnOnControlLoss = false,
        ) { error("enqueue rejected") }

        assertTrue(result.isFailure)
        assertEquals(oldGeneration, VpnRuntimeRegistry.snapshot().generation)
        assertEquals("running", VpnRuntimeRegistry.snapshot().state)
        assertTrue(coordinator.heartbeat("epoch-old", nowMillis = 4_999))
    }

    @Test
    fun staleRecoveryCleanupCannotStopInterleavedManualGeneration() {
        val coordinator = ControlLeaseCoordinator(timeoutMillis = 5_000)
        var recoveryStop: VpnRuntimeRegistry.StopRequest? = null

        val result = coordinator.recover(
            profileId = "profile-device-only",
            runtime = runtime,
            stopVpnOnControlLoss = false,
            onRollbackConflict = { recoveryStop = it },
        ) { generation ->
            assertTrue(VpnRuntimeRegistry.running("profile-device-only", runtime, generation))
            error("enqueue result became ambiguous")
        }
        assertTrue(result.isFailure)
        val replacement = VpnRuntimeRegistry.startRequested("profile-manual", runtime)
        var released = false

        assertFalse(
            VpnExternalStopCoordinator.completeActiveServiceStop(
                checkNotNull(recoveryStop),
                "stale recovery cleanup",
            ) { released = true },
        )
        assertFalse(released)
        assertTrue(VpnRuntimeRegistry.canStart(replacement))
    }

    @Test
    fun rejectedRecoveryCleanupCannotCloseInterleavedManualGeneration() {
        val idleGeneration = VpnRuntimeRegistry.snapshot().generation
        val replacement = VpnRuntimeRegistry.startRequested("profile-manual", runtime)
        var cleaned = false

        assertFalse(
            VpnRuntimeRegistry.cleanupRejectedRecoveryIfIdle(idleGeneration) {
                cleaned = true
            },
        )
        assertFalse(cleaned)
        assertTrue(VpnRuntimeRegistry.canStart(replacement))
    }

    @Test
    fun rejectedRecoveryCleansServiceResourcesWhileRegistryRemainsIdle() {
        val idleGeneration = VpnRuntimeRegistry.snapshot().generation
        var cleaned = false

        assertTrue(
            VpnRuntimeRegistry.cleanupRejectedRecoveryIfIdle(idleGeneration) {
                cleaned = true
            },
        )
        assertTrue(cleaned)
        assertEquals("stopped", VpnRuntimeRegistry.snapshot().state)
        assertEquals(idleGeneration, VpnRuntimeRegistry.snapshot().generation)
    }
}
