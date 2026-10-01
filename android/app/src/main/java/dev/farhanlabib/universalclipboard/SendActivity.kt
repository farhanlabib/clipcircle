package dev.farhanlabib.universalclipboard

import android.app.Activity
import android.content.ClipboardManager
import android.content.Intent
import android.os.Bundle
import android.widget.Toast

/**
 * Android only lets the app in front read the clipboard. This invisible screen
 * comes to the front, reads it (or takes text shared from another app), sends
 * it to the circle and closes.
 */
class SendActivity : Activity() {
    private var handled = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (intent?.action == Intent.ACTION_SEND) {
            val shared = intent.getStringExtra(Intent.EXTRA_TEXT)
            handled = true
            send(shared)
        }
    }

    // The clipboard can only be read once this window has focus.
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || handled) return
        handled = true
        val clipboard = getSystemService(ClipboardManager::class.java)
        val text = clipboard.primaryClip
            ?.takeIf { it.itemCount > 0 }
            ?.getItemAt(0)
            ?.coerceToText(this)
            ?.toString()
        send(text)
    }

    private fun send(text: String?) {
        if (text.isNullOrEmpty()) {
            toast("Nothing to send")
            finish()
            return
        }
        clipApp.background({
            if (!clipApp.node.isRunning()) clipApp.node.start()
            clipApp.node.sendText(text)
        }) { result ->
            toast(if (result.isSuccess) "Sent to your devices" else "Could not send: ${result.exceptionOrNull()?.message}")
            finish()
        }
    }

    private fun toast(message: String) = Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
}
