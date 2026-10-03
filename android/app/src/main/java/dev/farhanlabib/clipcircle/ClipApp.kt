package dev.farhanlabib.clipcircle

import android.app.Application
import android.content.ClipData
import android.content.ClipboardManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.util.Log
import java.util.concurrent.Executors
import uniffi.clip_ffi.ClipListener
import uniffi.clip_ffi.Node
import uniffi.clip_ffi.SharedFile

/** Owns the Rust node for the whole app. Node calls block, so use [worker]. */
class ClipApp : Application() {
    lateinit var node: Node
        private set

    /** Single background thread for every Node call. */
    val worker = Executors.newSingleThreadExecutor()

    /** Moves received files to Downloads without holding up the node. */
    private val saver = Executors.newSingleThreadExecutor()

    private val main = Handler(Looper.getMainLooper())

    /** When the circle last put something on the clipboard (elapsedRealtime). */
    @Volatile
    var ownClipAt = 0L
        private set

    /** Puts a clip from the circle on the clipboard. Main thread. */
    fun setOwnClip(clip: ClipData) {
        ownClipAt = SystemClock.elapsedRealtime()
        getSystemService(ClipboardManager::class.java).setPrimaryClip(clip)
    }

    /** Screens listen here to refresh when pairing finishes. */
    var onPairingResult: ((ok: Boolean, message: String) -> Unit)? = null

    override fun onCreate() {
        super.onCreate()
        val listener = object : ClipListener {
            override fun onClip(text: String) {
                main.post { setOwnClip(ClipData.newPlainText(LABEL_TEXT, text)) }
            }

            override fun onImage(png: ByteArray) {
                val uri = ClipImageProvider.save(this@ClipApp, png)
                main.post { setOwnClip(ClipData.newUri(contentResolver, LABEL_IMAGE, uri)) }
            }

            override fun onFiles(files: List<SharedFile>) {
                saver.execute {
                    runCatching { ReceivedFiles.save(this@ClipApp, files) }
                        .onSuccess { uris -> main.post { ReceivedFiles.announce(this@ClipApp, files.map { it.name }, uris) } }
                        .onFailure { Log.w("ClipApp", "could not save received files", it) }
                }
            }

            override fun onPaired(deviceName: String) {
                main.post { onPairingResult?.invoke(true, deviceName) }
            }

            override fun onPairingFailed(message: String) {
                main.post { onPairingResult?.invoke(false, message) }
            }
        }
        node = Node(filesDir.absolutePath, deviceName(), listener)
    }

    /** The name set in Settings > About phone, else maker and model. */
    private fun deviceName(): String =
        Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME)?.trim()?.takeIf { it.isNotEmpty() }
            ?: listOf(Build.MANUFACTURER, Build.MODEL).filterNot { it.isNullOrBlank() }.joinToString(" ")
                .ifEmpty { "Android" }

    companion object {
        const val LABEL_TEXT = "From your devices"
        const val LABEL_IMAGE = "Image from your devices"
        const val LABEL_FILES = "Files from your devices"

        /** Labels of clips the circle put on the clipboard, so they aren't sent back. */
        val OWN_LABELS = setOf(LABEL_TEXT, LABEL_IMAGE, LABEL_FILES)
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
