# Android JNI host wrapper

This directory contains the Kotlin owner wrapper for the Rust Mosh runtime. The native JNI symbols are implemented in compiler/src/jni_bridge.rs and exported by the libmoshc shared library.

## Build the Android native library

Requirements:
- Android NDK installed through Android Studio or the Android command-line tools.
- Rust Android target installed for each ABI being packaged.
- cargo-ndk installed.

For a 64-bit ARM Android device:

~~~sh
rustup target add aarch64-linux-android
cargo install cargo-ndk
cd compiler
cargo ndk -t arm64-v8a -o ../platform/android/src/main/jniLibs build --release --lib
~~~

The expected output is placed under platform/android/src/main/jniLibs/arm64-v8a/libmoshc.so. Build additional ABI directories only for targets that the host application actually supports.

## Use from an Android application

Copy or include MoshRuntime.kt in the package com.jusu.mosh, package the matching native library in jniLibs, then:

~~~kotlin
MoshRuntime().use { runtime ->
    val resultJson = runtime.eval("device.battery.is_supported()")
}
~~~

eval returns the C ABI JSON envelope as a string. The Kotlin wrapper serializes calls on each runtime instance and closes the native handle through AutoCloseable.

## Current limitations

- This wrapper exposes interpreter creation, evaluation, version, and cleanup.
- The default Rust backend reads Linux/Android procfs/sysfs telemetry where the Android process has access to it.
- It does not yet provide Android framework adapters for CameraX, Wi-Fi, Bluetooth, NFC, GPS, microphone, sensors, or permission prompts.
- No Android Gradle project or APK is produced by this repository change; integrate the Kotlin source and native library into an Android host application.
- Android NDK compilation and physical-device execution must be verified by an Android build environment. Desktop CI only checks the JNI Rust code on its host target.
