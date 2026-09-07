package com.interceptproxy.vpn

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidPlatformCompatibilityTest {
    @Test
    fun android7UsesLegacyServiceAndNotificationContract() {
        assertFalse(AndroidPlatformCompatibility.usesApi26ForegroundContract(24))
        assertFalse(AndroidPlatformCompatibility.usesApi26ForegroundContract(25))
    }

    @Test
    fun android8AndNewerUseApi26ForegroundContract() {
        assertTrue(AndroidPlatformCompatibility.usesApi26ForegroundContract(26))
        assertTrue(AndroidPlatformCompatibility.usesApi26ForegroundContract(36))
    }
}
