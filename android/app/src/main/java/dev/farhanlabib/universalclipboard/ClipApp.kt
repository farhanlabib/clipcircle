package dev.farhanlabib.universalclipboard

import android.app.Application
import android.content.ClipData
import android.content.ClipboardManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executors
import uniffi.clip_ffi.ClipListener
import uniffi.clip_ffi.Node

/** Owns the Rust node for the whole app. Node calls block, so use [worker]. */
class ClipApp : Application() {
    lateinit var node: Node
        private set

    /** Single background thread for every Node call. */
    val worker = Executors.newSingleThreadExecutor()

    private val main = Handler(Looper.getMainLooper())

    /** Screens listen here to refresh when pairing finishes. */
    var onPairingResult: ((ok: Boolean, message: String) -> Unit)? = null

    override fun onCreate() {
        super.onCreate()
        val listener = object : ClipListener {
            override fun onClip(text: String) {
                main.post {
                    val clipboard = getSystemService(ClipboardManager::class.java)
                    clipboard.setPrimaryClip(ClipData.newPlainText("From your devices", text))
                }
            }

            override fun onImage(png: ByteArray) {
                val uri = ClipImageProvider.save(this@ClipApp, png)
                main.post {
                    val clipboard = getSystemService(ClipboardManager::class.java)
                    clipboard.setPrimaryClip(ClipData.newUri(contentResolver, "Image from your devices", uri))
                }
            }

            override fun onPaired(deviceName: String) {
                main.post { onPairingResult?.invoke(true, deviceName) }
            }

            override fun onPairingFailed(message: String) {
                main.post { onPairingResult?.invoke(false, message) }
            }
        }
        node = Node(filesDir.absolutePath, Build.MODEL ?: "Android", listener)
    }

    /** Runs [work] on the worker, then [done] on the main thread with its result. */
    fun <T> background(work: () -> T, done: (Result<T>) -> Unit) {
        worker.execute {
            val result = runCatching(work)
            main.post { done(result) }
        }
    }
}

val android.content.Context.clipApp: ClipApp get() = applicationContext as ClipApp
