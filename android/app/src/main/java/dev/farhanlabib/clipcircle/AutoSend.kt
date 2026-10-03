package dev.farhanlabib.clipcircle

import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.PixelFormat
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.WindowManager

/**
 * Sends what's copied on this phone without a tap.
 *
 * Android only lets the app in focus read the clipboard, and doesn't tell a
 * background app that it changed. It does log a line each time it skips an
 * app's clipboard listener, so with READ_LOGS (granted once over adb) this
 * watches the log for its own package, then shows a 1-pixel window that takes
 * focus just long enough to read the clipboard. The same trick as KDE Connect.
 */
class AutoSend(private val context: Context) {
    private val main = Handler(Looper.getMainLooper())
    private val clipboard = context.getSystemService(ClipboardManager::class.java)
    private val windows = context.getSystemService(WindowManager::class.java)

    // Registered only so the clipboard service logs a line when it skips us.
    // When this app is in front the listener fires directly instead.
    private val listener = ClipboardManager.OnPrimaryClipChangedListener { changed() }

    private var logcat: Process? = null
    private var overlay: View? = null

    fun start() {
        if (logcat != null || !ready(context)) return
        clipboard.addPrimaryClipChangedListener(listener)
        val process = runCatching {
            // Only lines from now on (-T takes seconds since the epoch).
            val now = "%.3f".format(java.util.Locale.ROOT, System.currentTimeMillis() / 1000.0)
            ProcessBuilder("logcat", "-T", now, "-v", "brief", "ClipboardService:E", "*:S")
                .redirectErrorStream(true)
                .start()
        }.getOrElse {
            Log.w(TAG, "could not read the log", it)
            clipboard.removePrimaryClipChangedListener(listener)
            return
        }
        logcat = process
        val needle = "Denying clipboard access to ${context.packageName}"
        Thread({
            runCatching {
                process.inputStream.bufferedReader().useLines { lines ->
                    lines.filter { needle in it }.forEach { main.post { changed() } }
                }
            }
        }, "clip-logcat").apply {
            isDaemon = true
            start()
        }
    }

    fun stop() {
        clipboard.removePrimaryClipChangedListener(listener)
        logcat?.destroy()
        logcat = null
        close()
    }

    /** Main thread. */
    private fun changed() {
        // Our own read is in flight, or the change is a clip the circle just
        // put there.
        if (overlay != null || SystemClock.elapsedRealtime() - context.clipApp.ownClipAt < OWN_CLIP_QUIET_MS) return
        val view = object : View(context) {
            private var read = false

            override fun onWindowFocusChanged(hasFocus: Boolean) {
                super.onWindowFocusChanged(hasFocus)
                if (!hasFocus || read) return
                read = true
                val clip = runCatching { clipboard.primaryClip }.getOrNull()
                close()
                Outbox.sendClip(context, clip) { message ->
                    if (message != null && message != SENT) Log.w(TAG, message)
                }
            }
        }
        val params = WindowManager.LayoutParams(
            1,
            1,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            // Focusable (to read the clipboard), but touches go to the app below.
            WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
            PixelFormat.TRANSLUCENT,
        ).apply { gravity = Gravity.TOP or Gravity.START }
        runCatching { windows.addView(view, params) }.onFailure {
            Log.w(TAG, "could not show the overlay", it)
            return
        }
        overlay = view
        // Give the focus back even if it never arrives.
        main.postDelayed({ if (overlay === view) close() }, FOCUS_TIMEOUT_MS)
    }

    private fun close() {
        val view = overlay ?: return
        overlay = null
        runCatching { windows.removeView(view) }
    }

    companion object {
        private const val TAG = "AutoSend"
        private const val SENT = "Sent to your devices"
        private const val FOCUS_TIMEOUT_MS = 1500L
        private const val OWN_CLIP_QUIET_MS = 2000L

        /** The adb command that grants what reading the log needs. */
        fun grantCommand(context: Context) =
            "adb shell pm grant ${context.packageName} android.permission.READ_LOGS"

        fun canReadLog(context: Context) =
            context.checkSelfPermission(Manifest.permission.READ_LOGS) == PackageManager.PERMISSION_GRANTED

        fun canOverlay(context: Context) = Settings.canDrawOverlays(context)

        fun ready(context: Context) = canReadLog(context) && canOverlay(context)

        private fun prefs(context: Context) = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

        /** The user turned it on, after reading what it needs. Off by default. */
        fun enabled(context: Context) = prefs(context).getBoolean("auto_send", false)

        fun setEnabled(context: Context, on: Boolean) {
            prefs(context).edit().putBoolean("auto_send", on).apply()
        }
    }
}
