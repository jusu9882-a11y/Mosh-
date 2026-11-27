package com.jusu.mosh

/**
 * Kotlin owner for one native Mosh interpreter instance.
 *
 * Calls on a single instance are synchronized because the Rust interpreter
 * and its runtime handle are intentionally single-threaded.
 */
class MoshRuntime : AutoCloseable {
    private var handle: Long = nativeCreate()

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

    private external fun nativeCreate(): Long
    private external fun nativeFree(handle: Long)
    private external fun nativeEval(handle: Long, source: String): String
    private external fun nativeVersion(): String

    companion object {
        init {
            System.loadLibrary("moshc")
        }
    }
}
