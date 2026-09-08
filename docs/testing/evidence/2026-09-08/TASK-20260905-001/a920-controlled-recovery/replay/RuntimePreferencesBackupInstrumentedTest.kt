package com.interceptproxy.vpn

import android.app.Instrumentation
import android.content.Context
import android.content.SharedPreferences
import android.os.Bundle
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.io.FileOutputStream
import java.nio.charset.StandardCharsets

class RuntimePreferencesBackupInstrumentedTest {
    private val instrumentation: Instrumentation = InstrumentationRegistry.getInstrumentation()
    private val targetContext: Context = instrumentation.targetContext
    private val testContext: Context = instrumentation.context

    @Test
    fun backupOrRestoreRuntimePreferences() {
        val arguments = InstrumentationRegistry.getArguments()
        val action = arguments.getString(ARG_ACTION)
            ?: failWith("Missing instrumentation argument: action=backup|restore")
        val backupFile = resolveBackupFile(arguments.getString(ARG_BACKUP_FILE))

        when (action) {
            ACTION_BACKUP -> backup(backupFile)
            ACTION_RESTORE -> restore(backupFile)
            else -> failWith("Unsupported action '$action'; expected backup or restore")
        }
    }

    private fun backup(backupFile: File) {
        val preferenceFileExisted = preferenceFile().isFile
        val values = targetContext
            .getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
            .all
        val entries = JSONArray()

        values.keys.sorted().forEach { key ->
            val value = values[key] ?: failWith("Null SharedPreferences value for key '$key'")
            entries.put(encodeEntry(key, value))
        }

        val document = JSONObject()
            .put("schema_version", SCHEMA_VERSION)
            .put("target_package", targetContext.packageName)
            .put("preferences_name", PREFERENCES_NAME)
            .put("file_existed", preferenceFileExisted)
            .put("entries", entries)

        backupFile.parentFile?.let { parent ->
            check(parent.isDirectory || parent.mkdirs()) {
                "Cannot create backup directory: ${parent.absolutePath}"
            }
        }
        check(backupFile.createNewFile()) {
            "Refusing to overwrite existing backup: ${backupFile.absolutePath}"
        }
        try {
            FileOutputStream(backupFile, false).use { output ->
                output.write((document.toString(2) + "\n").toByteArray(StandardCharsets.UTF_8))
                output.fd.sync()
            }
        } catch (error: Throwable) {
            backupFile.delete()
            throw error
        }

        sendResult(
            result = "backup_complete",
            backupFile = backupFile,
            fileExisted = preferenceFileExisted,
            entryCount = values.size,
        )
    }

    private fun restore(backupFile: File) {
        check(backupFile.isFile) { "Backup file does not exist: ${backupFile.absolutePath}" }
        val document = JSONObject(backupFile.readText(StandardCharsets.UTF_8))
        require(document.getInt("schema_version") == SCHEMA_VERSION) {
            "Unsupported backup schema_version: ${document.get("schema_version")}"
        }
        require(document.getString("target_package") == targetContext.packageName) {
            "Backup target_package does not match ${targetContext.packageName}"
        }
        require(document.getString("preferences_name") == PREFERENCES_NAME) {
            "Backup preferences_name does not match $PREFERENCES_NAME"
        }

        val fileExisted = document.getBoolean("file_existed")
        val expected = decodeEntries(document.getJSONArray("entries"))

        if (!fileExisted) {
            targetContext.deleteSharedPreferences(PREFERENCES_NAME)
            check(!preferenceFile().exists()) {
                "Preferences file still exists after restoring the absent-file state"
            }
            check(
                targetContext.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE).all.isEmpty(),
            ) { "Preferences are not empty after restoring the absent-file state" }
        } else {
            val preferences = targetContext.getSharedPreferences(
                PREFERENCES_NAME,
                Context.MODE_PRIVATE,
            )
            check(writeExact(preferences, expected)) { "SharedPreferences commit returned false" }
            check(preferenceFile().isFile) {
                "Preferences file was not recreated although it existed at backup time"
            }
            verifyExact(preferences.all, expected)
        }

        sendResult(
            result = "restore_complete",
            backupFile = backupFile,
            fileExisted = fileExisted,
            entryCount = expected.size,
            readbackVerified = true,
        )
    }

    private fun writeExact(
        preferences: SharedPreferences,
        values: Map<String, PreferenceValue>,
    ): Boolean {
        if (values.isEmpty()) {
            val sentinel = "__intercept_proxy_backup_restore_empty_sentinel__"
            check(preferences.edit().clear().putString(sentinel, "1").commit()) {
                "SharedPreferences empty-file preparation commit returned false"
            }
            return preferences.edit().remove(sentinel).commit()
        }

        val editor = preferences.edit().clear()
        values.forEach { (key, value) ->
            when (value) {
                is PreferenceValue.StringValue -> editor.putString(key, value.value)
                is PreferenceValue.BooleanValue -> editor.putBoolean(key, value.value)
                is PreferenceValue.IntValue -> editor.putInt(key, value.value)
                is PreferenceValue.LongValue -> editor.putLong(key, value.value)
                is PreferenceValue.FloatValue -> editor.putFloat(key, value.value)
                is PreferenceValue.StringSetValue -> editor.putStringSet(key, value.value)
            }
        }
        return editor.commit()
    }

    private fun encodeEntry(key: String, value: Any): JSONObject = when (value) {
        is String -> entry(key, "String").put("value", value)
        is Boolean -> entry(key, "Boolean").put("value", value)
        is Int -> entry(key, "Int").put("value", value.toString())
        is Long -> entry(key, "Long").put("value", value.toString())
        is Float -> entry(key, "Float").put(
            "raw_bits",
            java.lang.Float.floatToRawIntBits(value)
                .toUInt()
                .toString(16)
                .padStart(8, '0'),
        )
        is Set<*> -> {
            require(value.all { it is String }) {
                "Unsupported non-String value in StringSet for key '$key'"
            }
            val items = JSONArray()
            value.map { it as String }.sorted().forEach(items::put)
            entry(key, "StringSet").put("value", items)
        }
        else -> failWith("Unsupported SharedPreferences type for key '$key': ${value.javaClass.name}")
    }

    private fun decodeEntries(entries: JSONArray): LinkedHashMap<String, PreferenceValue> {
        val decoded = linkedMapOf<String, PreferenceValue>()
        for (index in 0 until entries.length()) {
            val entry = entries.getJSONObject(index)
            val key = entry.getString("key")
            require(!decoded.containsKey(key)) { "Duplicate preference key in backup: '$key'" }
            decoded[key] = when (val type = entry.getString("type")) {
                "String" -> PreferenceValue.StringValue(entry.getString("value"))
                "Boolean" -> PreferenceValue.BooleanValue(entry.getBoolean("value"))
                "Int" -> PreferenceValue.IntValue(entry.getString("value").toInt())
                "Long" -> PreferenceValue.LongValue(entry.getString("value").toLong())
                "Float" -> {
                    val rawBits = entry.getString("raw_bits")
                    require(rawBits.matches(Regex("[0-9a-fA-F]{8}"))) {
                        "Invalid Float raw_bits for key '$key': '$rawBits'"
                    }
                    PreferenceValue.FloatValue(
                        java.lang.Float.intBitsToFloat(rawBits.toUInt(16).toInt()),
                    )
                }
                "StringSet" -> PreferenceValue.StringSetValue(
                    entry.getJSONArray("value").toStringSet(key),
                )
                else -> failWith("Unsupported preference type '$type' for key '$key'")
            }
        }
        return decoded
    }

    private fun verifyExact(actual: Map<String, *>, expected: Map<String, PreferenceValue>) {
        require(actual.keys == expected.keys) {
            "Restored keys differ: expected=${expected.keys.sorted()}, actual=${actual.keys.sorted()}"
        }
        expected.forEach { (key, expectedValue) ->
            val actualValue = actual[key]
                ?: failWith("Restored value is null or missing for key '$key'")
            val matches = when (expectedValue) {
                is PreferenceValue.StringValue -> actualValue is String && actualValue == expectedValue.value
                is PreferenceValue.BooleanValue -> actualValue is Boolean && actualValue == expectedValue.value
                is PreferenceValue.IntValue -> actualValue is Int && actualValue == expectedValue.value
                is PreferenceValue.LongValue -> actualValue is Long && actualValue == expectedValue.value
                is PreferenceValue.FloatValue -> actualValue is Float &&
                    java.lang.Float.floatToRawIntBits(actualValue) ==
                    java.lang.Float.floatToRawIntBits(expectedValue.value)
                is PreferenceValue.StringSetValue -> actualValue is Set<*> &&
                    actualValue.all { it is String } && actualValue == expectedValue.value
            }
            require(matches) {
                "Restored value differs for key '$key': expected type=${expectedValue.typeName}, " +
                    "actual type=${actualValue.javaClass.name}"
            }
        }
    }

    private fun JSONArray.toStringSet(key: String): Set<String> {
        val values = linkedSetOf<String>()
        for (index in 0 until length()) {
            val value = getString(index)
            require(values.add(value)) { "Duplicate StringSet value for key '$key': '$value'" }
        }
        return values
    }

    private fun entry(key: String, type: String): JSONObject = JSONObject()
        .put("key", key)
        .put("type", type)

    private fun resolveBackupFile(argument: String?): File {
        val fileName = argument ?: DEFAULT_BACKUP_FILE
        require(fileName.isNotBlank() && fileName == File(fileName).name) {
            "$ARG_BACKUP_FILE must be a non-empty file name without directory components"
        }
        val externalFiles = requireNotNull(testContext.getExternalFilesDir(null)) {
            "Instrumentation testContext externalFilesDir is unavailable"
        }
        return File(externalFiles, fileName)
    }

    private fun preferenceFile(): File = File(
        File(targetContext.applicationInfo.dataDir, "shared_prefs"),
        "$PREFERENCES_NAME.xml",
    )

    private fun sendResult(
        result: String,
        backupFile: File,
        fileExisted: Boolean,
        entryCount: Int,
        readbackVerified: Boolean? = null,
    ) {
        instrumentation.sendStatus(
            STATUS_CODE,
            Bundle().apply {
                putString("result", result)
                putString("backup_path", backupFile.absolutePath)
                putString("preferences_name", PREFERENCES_NAME)
                putString("file_existed", fileExisted.toString())
                putString("entry_count", entryCount.toString())
                readbackVerified?.let {
                    putString("readback_verified", it.toString())
                }
            },
        )
    }

    private fun failWith(message: String): Nothing {
        fail(message)
        error(message)
    }

    private sealed class PreferenceValue(val typeName: String) {
        data class StringValue(val value: String) : PreferenceValue("String")
        data class BooleanValue(val value: Boolean) : PreferenceValue("Boolean")
        data class IntValue(val value: Int) : PreferenceValue("Int")
        data class LongValue(val value: Long) : PreferenceValue("Long")
        data class FloatValue(val value: Float) : PreferenceValue("Float")
        data class StringSetValue(val value: Set<String>) : PreferenceValue("StringSet")
    }

    companion object {
        private const val SCHEMA_VERSION = 1
        private const val STATUS_CODE = 0
        private const val ARG_ACTION = "action"
        private const val ARG_BACKUP_FILE = "backup_file"
        private const val ACTION_BACKUP = "backup"
        private const val ACTION_RESTORE = "restore"
        private const val DEFAULT_BACKUP_FILE = "intercept_proxy_vpn_runtime.backup.json"
        private const val PREFERENCES_NAME = "intercept_proxy_vpn_runtime"
    }
}
