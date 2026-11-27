package com.jusu.mosh

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build

/**
 * Narrow Android framework adapter used by the Rust JNI backend.
 * Battery data is read from Android system services; no runtime permission
 * is required for these battery properties.
 */
class AndroidDeviceHost(context: Context) {
    private val appContext = context.applicationContext
    private val batteryManager =
        appContext.getSystemService(Context.BATTERY_SERVICE) as? BatteryManager

    fun batterySupported(): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP && batteryManager != null

    private fun batteryIntent(): Intent? =
        appContext.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))

    fun batteryAvailable(): Boolean {
        val intent = batteryIntent() ?: return false
        return intent.getBooleanExtra(BatteryManager.EXTRA_PRESENT, true) &&
            intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) >= 0
    }

    fun batteryData(): String {
        val intent = batteryIntent()
            ?: return "present=false"
        val present = intent.getBooleanExtra(BatteryManager.EXTRA_PRESENT, true)
        if (!present) return "present=false"

        val level = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
        val capacity = if (level >= 0 && scale > 0) {
            (level.toDouble() * 100.0 / scale.toDouble()).toString()
        } else {
            ""
        }

        val state = when (intent.getIntExtra(BatteryManager.EXTRA_STATUS, -1)) {
            BatteryManager.BATTERY_STATUS_CHARGING -> "Charging"
            BatteryManager.BATTERY_STATUS_DISCHARGING -> "Discharging"
            BatteryManager.BATTERY_STATUS_FULL -> "Full"
            BatteryManager.BATTERY_STATUS_NOT_CHARGING -> "Not charging"
            else -> ""
        }

        val voltage = intent.getIntExtra(BatteryManager.EXTRA_VOLTAGE, -1)
            .takeIf { it >= 0 }?.toString() ?: ""
        val temperature = intent.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, -1)
            .takeIf { it >= 0 }?.let { (it / 10.0).toString() } ?: ""
        val currentMicroAmps = batteryManager?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW)
            ?: Int.MIN_VALUE
        val currentMa = if (currentMicroAmps != Int.MIN_VALUE) {
            (currentMicroAmps / 1000.0).toString()
        } else {
            ""
        }
        val technology = intent.getStringExtra(BatteryManager.EXTRA_TECHNOLOGY).orEmpty()

        return listOf(
            "present=true",
            "capacity_percent=$capacity",
            "state=$state",
            "voltage_mv=$voltage",
            "current_ma=$currentMa",
            "temperature_c=$temperature",
            "technology=$technology"
        ).joinToString(";")
    }
}
