# Android host integration

This directory contains the Kotlin owner wrapper for the Rust Mosh runtime and an Android library module for real Android framework integrations.

## Build the Android host library

Requirements:
- JDK 17
- Android SDK platform 35 and Android build tools
- Gradle 8.11.1 (the CI workflow uses this version)

From this directory:

~~~sh
gradle :mosh-android:assembleDebug
~~~

The library module compiles the Kotlin sources in `src/main/kotlin` and declares CameraX 1.7.0. The merged library manifest requests the CAMERA permission and declares camera hardware as optional, so apps without a camera can still install the library.

## Build the Android native Mosh library

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

The expected output is placed under `platform/android/src/main/jniLibs/arm64-v8a/libmoshc.so`. Build additional ABI directories only for targets that the host application actually supports.

## Use the Mosh runtime

Include `MoshRuntime.kt` and `AndroidDeviceHost.kt` in the package `com.jusu.mosh`, package the matching native library in `jniLibs`, then:

~~~kotlin
MoshRuntime(applicationContext).use { runtime ->
    val resultJson = runtime.eval("device.battery.is_supported()")
}
~~~

`eval` returns the C ABI JSON envelope as a string. Calls on one runtime instance are serialized, and closing the wrapper releases the native runtime handle.

## Use CameraX from an Android host

The `CameraXDevice` class is a real CameraX preview and image-analysis session. Create it from an Activity before its lifecycle reaches STARTED, pass a `PreviewView`, and call `open()`. The class registers a runtime CAMERA permission launcher, binds preview and analysis use cases to the supplied lifecycle, and closes its resources when the lifecycle is destroyed.

~~~kotlin
private lateinit var camera: CameraXDevice

override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    val preview = PreviewView(this)
    setContentView(preview)

    camera = CameraXDevice(
        activity = this,
        lifecycleOwner = this,
        previewView = preview,
        onFrame = { frame ->
            // frame.luma is a copied Y (luminance) plane.
            // rowStride and pixelStride describe its memory layout.
        },
        onError = { error -> android.util.Log.e("MoshCamera", "Camera error", error) }
    )
    camera.open(CameraXDevice.Lens.BACK)
}

override fun onDestroy() {
    camera.close()
    super.onDestroy()
}
~~~

The callback receives frame dimensions, rotation, timestamp, row stride, pixel stride, and a copied luminance plane. CameraX uses `STRATEGY_KEEP_ONLY_LATEST`; every `ImageProxy` is closed by the analyzer, and only the two use cases owned by this class are unbound. The host remains responsible for deciding what to do with frame data and for providing a visible preview surface.

## Current implementation boundaries

- Implemented in this Android library: CameraX capability check, runtime CAMERA permission request, back/front lens selection, preview, bounded latest-frame analysis, copied luma-plane delivery, lifecycle pause/restart, and explicit close.
- The CameraX session is currently a Kotlin host API. It is **not yet exposed as `device.camera.*` Mosh expressions**; the current JNI evaluator is synchronous and needs an explicit native-to-host asynchronous resource bridge before that can be done safely.
- Android framework battery telemetry is available through `AndroidDeviceHost` and the JNI backend.
- The default Rust backend reads Linux/Android procfs/sysfs memory, CPU, and storage telemetry where the process has access.
- Wi-Fi, Bluetooth, NFC, GPS, microphone, sensors, and other hardware adapters are not implemented yet.
- The Gradle CI job compiles the Android library, but it does not perform Android NDK cross-compilation, create an APK, or run on a physical device. Those validations remain necessary before claiming device-level operation.
