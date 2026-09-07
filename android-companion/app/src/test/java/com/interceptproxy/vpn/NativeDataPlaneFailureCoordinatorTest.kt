package com.interceptproxy.vpn

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class NativeDataPlaneFailureCoordinatorTest {
    @Test
    fun delayedOldGenerationFailureCannotFaultOrTearDownReplacement() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val oldGeneration = VpnRuntimeRegistry.startRequested("profile-old", runtime)
        val replacement = VpnRuntimeRegistry.startRequested("profile-new", runtime)
        var tornDown = false

        val handled = NativeDataPlaneFailureCoordinator.failOpenIfCurrent(
            oldGeneration,
            "old runtime failed",
        ) { tornDown = true }

        assertFalse(handled)
        assertFalse(tornDown)
        assertTrue(VpnRuntimeRegistry.canStart(replacement))
        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun currentGenerationFailureFaultsAndTearsDownImmediately() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val generation = VpnRuntimeRegistry.startRequested("profile", runtime)
        var tornDown = false

        val handled = NativeDataPlaneFailureCoordinator.failOpenIfCurrent(
            generation,
            "current runtime failed",
        ) { tornDown = true }

        assertTrue(handled)
        assertTrue(tornDown)
        assertFalse(VpnRuntimeRegistry.canStart(generation))
        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun replacementStartCannotEnterBetweenCurrentFailureCheckAndTeardown() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val generation = VpnRuntimeRegistry.startRequested("profile-old", runtime)
        val teardownEntered = CountDownLatch(1)
        val releaseTeardown = CountDownLatch(1)
        val replacementAttempted = CountDownLatch(1)
        val replacementCompleted = CountDownLatch(1)

        val failure = Thread {
            NativeDataPlaneFailureCoordinator.failOpenIfCurrent(generation, "failed") {
                teardownEntered.countDown()
                check(releaseTeardown.await(1, TimeUnit.SECONDS))
            }
        }
        failure.start()
        assertTrue(teardownEntered.await(1, TimeUnit.SECONDS))

        val replacement = Thread {
            replacementAttempted.countDown()
            VpnRuntimeRegistry.startRequested("profile-new", runtime)
            replacementCompleted.countDown()
        }
        replacement.start()
        assertTrue(replacementAttempted.await(1, TimeUnit.SECONDS))
        assertFalse(replacementCompleted.await(100, TimeUnit.MILLISECONDS))

        releaseTeardown.countDown()
        failure.join(1_000)
        replacement.join(1_000)
        assertTrue(replacementCompleted.await(1, TimeUnit.SECONDS))
        VpnRuntimeRegistry.resetForTest()
    }
}
