package com.jusu.mosh

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.util.Size
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import java.io.Closeable
import java.nio.ByteBuffer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * A real CameraX preview + analysis session owned by an Android host Activity.
 *
 * The consumer receives a copied luma plane, never an ImageProxy. Every ImageProxy
 * is closed before the analyzer returns, so consumers cannot stall CameraX by
 * retaining framework-owned buffers. ImageAnalysis uses KEEP_ONLY_LATEST to bound
 * queued work and memory pressure.
 *
 * Construct this before the Activity reaches STARTED because the permission
 * launcher must be registered before the Activity Result registry is started.
 */
class CameraXDevice(
    private val activity: ComponentActivity,
    private val lifecycleOwner: LifecycleOwner,
    private val previewView: PreviewView,
    private val onFrame: (CameraFrame) -> Unit,
    private val onError: (Throwable) -> Unit = {}
) : Closeable, DefaultLifecycleObserver {
    enum class Lens { BACK, FRONT }

    data class CameraFrame(
        val width: Int,
        val height: Int,
        val rotationDegrees: Int,
        val timestampNanos: Long,
        val rowStride: Int,
        val pixelStride: Int,
        val luma: ByteArray
    )

    private val appContext: Context = activity.applicationContext
    private val analysisExecutor: ExecutorService = Executors.newSingleThreadExecutor()
    private val closed = AtomicBoolean(false)
    private val mainExecutor = ContextCompat.getMainExecutor(appContext)
    private val permissionLauncher: ActivityResultLauncher<String> =
        activity.registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) bindCamera() else onError(SecurityException("Camera permission was denied"))
        }

    @Volatile private var provider: ProcessCameraProvider? = null
    @Volatile private var preview: Preview? = null
    @Volatile private var analysis: ImageAnalysis? = null
    @Volatile private var selectedLens: Lens = Lens.BACK
    @Volatile private var targetSize: Size = Size(1280, 720)
    @Volatile private var requested: Boolean = false

    init {
        lifecycleOwner.lifecycle.addObserver(this)
        previewView.implementationMode = PreviewView.ImplementationMode.PERFORMANCE
        previewView.scaleType = PreviewView.ScaleType.FILL_CENTER
    }

    fun isSupported(): Boolean =
        appContext.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)

    fun hasPermission(): Boolean =
        ContextCompat.checkSelfPermission(appContext, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED

    /** Requests CAMERA permission through the Activity Result API, then binds on approval. */
    fun open(lens: Lens = Lens.BACK, resolution: Size = Size(1280, 720)) {
        check(!closed.get()) { "Camera session is closed" }
        selectedLens = lens
        targetSize = resolution
        requested = true
        if (!isSupported()) {
            onError(UnsupportedOperationException("This device has no camera feature"))
            return
        }
        if (hasPermission()) bindCamera() else permissionLauncher.launch(Manifest.permission.CAMERA)
    }

    private fun bindCamera() {
        if (closed.get()) return
        if (!hasPermission()) {
            onError(SecurityException("Camera permission is not granted"))
            return
        }
        val future = ProcessCameraProvider.getInstance(appContext)
        future.addListener({
            if (closed.get()) return@addListener
            try {
                val cameraProvider = future.get()
                provider = cameraProvider
                val previewUseCase = Preview.Builder().build().also {
                    it.surfaceProvider = previewView.surfaceProvider
                }
                val analysisUseCase = ImageAnalysis.Builder()
                    .setTargetResolution(targetSize)
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_YUV_420_888)
                    .build()
                    .also { useCase ->
                        useCase.setAnalyzer(analysisExecutor) { image -> analyze(image) }
                    }
                val selector = when (selectedLens) {
                    Lens.BACK -> CameraSelector.DEFAULT_BACK_CAMERA
                    Lens.FRONT -> CameraSelector.DEFAULT_FRONT_CAMERA
                }
                unbindOwnedUseCases(cameraProvider)
                cameraProvider.bindToLifecycle(lifecycleOwner, selector, previewUseCase, analysisUseCase)
                preview = previewUseCase
                analysis = analysisUseCase
            } catch (error: Throwable) {
                onError(error)
            }
        }, mainExecutor)
    }

    private fun analyze(image: ImageProxy) {
        try {
            if (closed.get()) return
            val plane = image.planes.firstOrNull()
                ?: throw IllegalStateException("CameraX supplied no image planes")
            val luma = copyPlane(plane.buffer)
            onFrame(
                CameraFrame(
                    width = image.width,
                    height = image.height,
                    rotationDegrees = image.imageInfo.rotationDegrees,
                    timestampNanos = image.imageInfo.timestamp,
                    rowStride = plane.rowStride,
                    pixelStride = plane.pixelStride,
                    luma = luma
                )
            )
        } catch (error: Throwable) {
            if (!closed.get()) onError(error)
        } finally {
            image.close()
        }
    }

    private fun copyPlane(source: ByteBuffer): ByteArray {
        val buffer = source.duplicate()
        val bytes = ByteArray(buffer.remaining())
        buffer.get(bytes)
        return bytes
    }

    private fun unbindOwnedUseCases(cameraProvider: ProcessCameraProvider) {
        val owned = listOfNotNull(preview, analysis)
        if (owned.isNotEmpty()) cameraProvider.unbind(*owned.toTypedArray())
        preview = null
        analysis = null
    }

    /** Stops camera use cases and prevents automatic restart until open() is called again. */
    fun stop() {
        requested = false
        val cameraProvider = provider ?: return
        mainExecutor.execute { unbindOwnedUseCases(cameraProvider) }
    }

    override fun onStop(owner: LifecycleOwner) {
        val cameraProvider = provider ?: return
        mainExecutor.execute { unbindOwnedUseCases(cameraProvider) }
    }

    override fun onStart(owner: LifecycleOwner) {
        if (!closed.get() && requested && hasPermission() && preview == null) bindCamera()
    }

    override fun onDestroy(owner: LifecycleOwner) {
        close()
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        lifecycleOwner.lifecycle.removeObserver(this)
        val cameraProvider = provider
        if (cameraProvider != null) {
            mainExecutor.execute {
                unbindOwnedUseCases(cameraProvider)
                provider = null
            }
        }
        analysisExecutor.shutdown()
    }
}
