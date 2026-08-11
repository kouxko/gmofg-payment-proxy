package com.interceptproxy.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class VpnServiceResourceOrderTest {
    @Test
    fun foregroundStartsAfterExternalControlIsAttached() {
        val events = mutableListOf<String>()

        VpnServiceResourceOrder.startService(
            attachExternalControl = { events += "attach_external_control" },
            startForeground = { events += "start_foreground" },
        )

        assertEquals(listOf("attach_external_control", "start_foreground"), events)
    }

    @Test
    fun revokeStopsVpnBeforeCallingSystemLifecycle() {
        val events = mutableListOf<String>()

        VpnServiceResourceOrder.revokeService(
            stopVpn = { events += "stop_vpn" },
            revokeSystemPermission = { events += "revoke_system_permission" },
        )

        assertEquals(listOf("stop_vpn", "revoke_system_permission"), events)
    }

    @Test
    fun closeCurrentDataPlaneReleasesTunBeforeStoppingNativeRuntime() {
        val events = mutableListOf<String>()

        VpnServiceResourceOrder.closeCurrentDataPlane(
            releaseTun = { events += "release_tun" },
            clearTunConfiguration = { events += "clear_tun_configuration" },
            clearActiveGeneration = { events += "clear_active_generation" },
            stopNativeDataPlane = { events += "stop_native_data_plane" },
        )

        assertEquals(
            listOf(
                "release_tun",
                "clear_tun_configuration",
                "clear_active_generation",
                "stop_native_data_plane",
            ),
            events,
        )
    }

    @Test
    fun destroyUnregistersReceiverBeforeClosingTunAndNativeRuntime() {
        val events = mutableListOf<String>()

        VpnServiceResourceOrder.destroyService(
            unregisterPackageReceiver = { events += "unregister_receiver" },
            releaseTun = { events += "release_tun" },
            stopNativeDataPlane = { events += "stop_native_data_plane" },
            clearActiveGeneration = { events += "clear_active_generation" },
        )

        assertEquals(
            listOf(
                "unregister_receiver",
                "release_tun",
                "stop_native_data_plane",
                "clear_active_generation",
            ),
            events,
        )
    }

    @Test
    fun foregroundStopsBeforeServiceInstance() {
        val events = mutableListOf<String>()

        VpnServiceResourceOrder.finishStopping(
            stopForeground = { events += "stop_foreground" },
            stopService = { events += "stop_service" },
        )

        assertEquals(listOf("stop_foreground", "stop_service"), events)
    }
}
