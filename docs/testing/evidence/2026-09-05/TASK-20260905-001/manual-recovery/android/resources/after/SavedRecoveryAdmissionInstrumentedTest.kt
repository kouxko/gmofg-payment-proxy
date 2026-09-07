package com.interceptproxy.vpn

import android.content.Context
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class SavedRecoveryAdmissionInstrumentedTest {
    private val context: Context = InstrumentationRegistry.getInstrumentation().targetContext

    @Before
    fun resetState() {
        ControlLeaseManager.clear()
        VpnRuntimeRegistry.resetForTest()
    }

    @After
    fun clearState() {
        ControlLeaseManager.clear()
        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun protectedSavedActivationWaitsWithoutStartingService() {
        var enqueued = false

        val result = InterceptVpnServiceControl.recoverSavedActivation(
            context,
            activation(stopVpnOnControlLoss = true),
        ) { enqueued = true }

        assertEquals(RecoveryAdmission.WaitingForDesktopControl, result.getOrThrow())
        assertFalse(enqueued)
        assertEquals("stopped", VpnRuntimeRegistry.snapshot().state)
        assertEquals(0L, VpnRuntimeRegistry.snapshot().generation)
    }

    @Test
    fun unprotectedSavedActivationKeepsDeviceOnlyRecovery() {
        var enqueuedGeneration: Long? = null

        val result = InterceptVpnServiceControl.recoverSavedActivation(
            context,
            activation(stopVpnOnControlLoss = false),
        ) { generation -> enqueuedGeneration = generation }

        val started = result.getOrThrow() as RecoveryAdmission.Started
        assertEquals(started.generation, enqueuedGeneration)
        assertTrue(VpnRuntimeRegistry.canStart(started.generation))
        assertFalse(ControlLeaseManager.heartbeat("unmanaged-recovery"))
    }

    private fun activation(stopVpnOnControlLoss: Boolean): StoredActivation {
        val profile = JSONObject()
            .put("id", "profile-saved")
            .put("target_applications", JSONArray())
            .put("proxy_routes", JSONArray())
            .put("stop_vpn_on_control_loss", stopVpnOnControlLoss)
        val routes = JSONArray()
        val runtime = JSONObject()
            .put("routes", routes)
            .put("route_source", JSONArray())
            .put(
                "profile_fingerprint",
                ProxyRuntimeParser.sha256(ProxyRuntimeParser.canonicalJson(profile)),
            )
            .put(
                "route_fingerprint",
                ProxyRuntimeParser.sha256(ProxyRuntimeParser.canonicalJson(routes)),
            )
            .put("route_count", 0)
        return StoredActivation(profile.toString(), runtime.toString())
    }
}
