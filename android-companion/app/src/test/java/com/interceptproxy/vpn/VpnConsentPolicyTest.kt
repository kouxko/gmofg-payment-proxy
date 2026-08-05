package com.interceptproxy.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class VpnConsentPolicyTest {
    @Test
    fun alreadyGrantedPermissionOnlyFinishesAuthorizationActivity() {
        assertEquals(VpnConsentNextStep.Finish, VpnConsentPolicy.afterPrepare(false))
    }

    @Test
    fun authorizationResultNeverStartsVpnService() {
        assertEquals(VpnConsentNextStep.Finish, VpnConsentPolicy.afterResult(granted = true))
        assertEquals(VpnConsentNextStep.Finish, VpnConsentPolicy.afterResult(granted = false))
    }
}
