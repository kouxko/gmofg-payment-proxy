package com.interceptproxy.vpn

import org.junit.Assert.assertEquals
import org.junit.Test

class PackageInventoryTest {
    @Test
    fun `快照只保存包名与 UID`() {
        val snapshot = PackageInventory.snapshot("com.example.target", 10001)

        assertEquals("com.example.target", snapshot.packageName)
        assertEquals(10001, snapshot.uid)
    }
}
