package com.interceptproxy.vpn

import android.content.Context
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class RuntimeStateStoreInstrumentedTest {
    private val context: Context = InstrumentationRegistry.getInstrumentation().targetContext

    @After
    fun clearPreferences() {
        context.getSharedPreferences(RuntimeStateStore.PREFERENCES, Context.MODE_PRIVATE)
            .edit().clear().commit()
    }

    @Test
    fun activationRoundTripsAsOneRecord() {
        val activation = activation()
        val store = RuntimeStateStore(context)

        store.activation = activation

        assertEquals(activation, RuntimeStateStore(context).activation)
    }

    @Test
    fun tornActivationIsRejectedAndRemoved() {
        context.getSharedPreferences(RuntimeStateStore.PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putString(
                RuntimeStateStore.KEY_ACTIVATION,
                JSONObject().put("profile", JSONObject(activation().profileJson)).toString(),
            )
            .commit()

        assertNull(RuntimeStateStore(context).activation)
        assertNull(
            context.getSharedPreferences(RuntimeStateStore.PREFERENCES, Context.MODE_PRIVATE)
                .getString(RuntimeStateStore.KEY_ACTIVATION, null),
        )
    }

    @Test
    fun temporaryProxyRuntimeIsNeverPersistedForReboot() {
        val store = RuntimeStateStore(context)
        store.autoResumeEnabled = true

        store.activation = activation(withProxyRoute = true)

        assertNull(store.activation)
        assertFalse(store.autoResumeEnabled)
    }

    private fun activation(withProxyRoute: Boolean = false): StoredActivation {
        val route = JSONObject()
            .put("listener_id", "listener-1")
            .put("original_destination", "example.test")
            .put("original_ports", JSONArray().put(443))
        val routeSource = JSONArray().also { routes ->
            if (withProxyRoute) routes.put(route)
        }
        val routes = JSONArray().also { normalized ->
            if (withProxyRoute) {
                normalized.put(
                    JSONObject(route.toString())
                        .put("resolved_original_ips", JSONArray().put("203.0.113.10"))
                        .put("proxy_host", "127.0.0.1")
                        .put("proxy_port", 41_627),
                )
            }
        }
        val profile = JSONObject()
            .put("id", "profile-1")
            .put("target_applications", JSONArray())
            .put("proxy_routes", routeSource)
        val runtime = JSONObject()
            .put("routes", routes)
            .put("route_source", routeSource)
            .put(
                "profile_fingerprint",
                ProxyRuntimeParser.sha256(ProxyRuntimeParser.canonicalJson(profile)),
            )
            .put(
                "route_fingerprint",
                ProxyRuntimeParser.sha256(ProxyRuntimeParser.canonicalJson(routes)),
            )
            .put("route_count", routes.length())
        return StoredActivation(profile.toString(), runtime.toString())
    }
}
