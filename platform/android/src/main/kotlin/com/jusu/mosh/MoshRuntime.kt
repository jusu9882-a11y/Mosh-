package com.jusu.mosh

import android.content.Context

/**
 * Kotlin owner for one native Mosh interpreter instance.
 *
 * Calls on a single instance are synchronized because the Rust interpreter
 * and its runtime handle are intentionally single-threaded.
 */
class MoshRuntime(context: Context) : AutoCloseable {
    private val deviceHost = AndroidDeviceHost(context.applicationContext)
    private var handle: Long = nativeCreate(deviceHost)

    init {
        check(handle != 0L) { "Unable to create the native Mosh runtime" }
    }

    @Synchronized
    fun eval(source: String): String {
        check(handle != 0L) { "Mosh runtime is closed" }
        return nativeEval(handle, source)
    }

    fun version(): String = nativeVersion()

    @Synchronized
    override fun close() {
        if (handle != 0L) {
            nativeFree(handle)
            handle = 0L
        }
    }

    private external fun nativeCreate(host: AndroidDeviceHost): Long
    private external fun nativeFree(handle: Long)
    private external fun nativeEval(handle: Long, source: String): String
    private external fun nativeVersion(): String

    companion object {
        init {
            System.loadLibrary("moshc")
        }
    }
}
