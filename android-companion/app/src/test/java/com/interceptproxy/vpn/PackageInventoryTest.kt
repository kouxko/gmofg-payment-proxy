package com.interceptproxy.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PackageInventoryTest {
    @Test
    fun `签名读取失败仍保留包名与 UID`() {
        val snapshot = PackageInventory.snapshot("com.example.unreadable", 10001, null)

        assertEquals("com.example.unreadable", snapshot.packageName)
        assertEquals(10001, snapshot.uid)
        assertTrue(snapshot.signingSha256.isEmpty())
    }

    @Test
    fun `空 signer 列表按不可验证处理`() {
        val snapshot = PackageInventory.snapshot("com.example.empty", 10002, emptyList())

        assertTrue(snapshot.signingSha256.isEmpty())
    }
}
