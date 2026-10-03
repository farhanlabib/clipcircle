package dev.farhanlabib.clipcircle

import android.app.Activity
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast

/**
 * Android only lets the app in front read the clipboard. This invisible screen
 * comes to the front, reads it (or takes text, an image or files shared from
 * another app), sends it to the circle and closes.
 */
class SendActivity : Activity() {
    private var handled = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val intent = intent ?: return
        when (intent.action) {
            Intent.ACTION_SEND -> {
                handled = true
                val stream = streamOf(intent)
                when {
                    stream == null -> Outbox.sendText(this, intent.getStringExtra(Intent.EXTRA_TEXT), ::done)
                    intent.type?.startsWith("image/") == true -> Outbox.sendImage(this, stream, ::done)
                    else -> Outbox.sendFiles(this, listOf(stream), ::done)
                }
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                handled = true
                Outbox.sendFiles(this, streamsOf(intent), ::done)
            }
        }
    }

    // The clipboard can only be read once this window has focus.
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || handled) return
        handled = true
        Outbox.sendClip(this, getSystemService(ClipboardManager::class.java).primaryClip, ::done)
    }

    private fun streamOf(intent: Intent): Uri? =
        if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(Intent.EXTRA_STREAM)
        }

    private fun streamsOf(intent: Intent): List<Uri> =
        if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)
        } ?: emptyList()

    private fun done(message: String?) {
        Toast.makeText(this, message ?: "Nothing new to send", Toast.LENGTH_SHORT).show()
        finish()
    }
}
