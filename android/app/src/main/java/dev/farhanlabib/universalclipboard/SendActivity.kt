package dev.farhanlabib.universalclipboard

import android.app.Activity
import android.content.ClipboardManager
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import java.io.ByteArrayOutputStream

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
                    stream == null -> sendText(intent.getStringExtra(Intent.EXTRA_TEXT))
                    intent.type?.startsWith("image/") == true -> sendImage(stream)
                    else -> sendFiles(listOf(stream))
                }
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                handled = true
                sendFiles(streamsOf(intent))
            }
        }
    }

    // The clipboard can only be read once this window has focus.
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || handled) return
        handled = true
        val clipboard = getSystemService(ClipboardManager::class.java)
        val clip = clipboard.primaryClip?.takeIf { it.itemCount > 0 }
        if (clip == null) {
            sendText(null)
            return
        }
        val item = clip.getItemAt(0)
        val uri = item.uri
        when {
            uri != null && clip.description.hasMimeType("image/*") -> sendImage(uri)
            // Copied files: URIs without text.
            uri != null && item.text == null ->
                sendFiles((0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri })
            else -> sendText(item.coerceToText(this)?.toString())
        }
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

    private fun sendFiles(uris: List<Uri>) {
        if (uris.isEmpty()) {
            done("Nothing to send")
            return
        }
        send { clipApp.node.sendFiles(ReceivedFiles.copyForSending(this, uris)) }
    }

    private fun sendText(text: String?) {
        if (text.isNullOrEmpty()) {
            done("Nothing to send")
            return
        }
        send { clipApp.node.sendText(text) }
    }

    // Read before finishing, while this screen still holds access to the URI.
    private fun sendImage(uri: Uri) = send { clipApp.node.sendImage(readAsPng(uri)) }

    private fun send(work: () -> Unit) {
        clipApp.background({
            if (!clipApp.node.isRunning()) clipApp.node.start()
            work()
        }) { result ->
            done(if (result.isSuccess) "Sent to your devices" else "Could not send: ${result.exceptionOrNull()?.message}")
        }
    }

    /** PNG bytes for the image at [uri], scaled down if it is very large. */
    private fun readAsPng(uri: Uri): ByteArray {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        contentResolver.openInputStream(uri)!!.use { BitmapFactory.decodeStream(it, null, bounds) }
        val longest = maxOf(bounds.outWidth, bounds.outHeight)
        require(longest > 0) { "not an image" }
        if (bounds.outMimeType == "image/png" && longest <= MAX_SIDE) {
            return contentResolver.openInputStream(uri)!!.use { it.readBytes() }
        }
        var sample = 1
        while (longest / sample > MAX_SIDE) sample *= 2
        val options = BitmapFactory.Options().apply { inSampleSize = sample }
        val bitmap = contentResolver.openInputStream(uri)!!.use { BitmapFactory.decodeStream(it, null, options) }
            ?: error("not an image")
        return ByteArrayOutputStream().use { out ->
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
            bitmap.recycle()
            out.toByteArray()
        }
    }

    private fun done(message: String) {
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
        finish()
    }

    private companion object {
        /** Photos are scaled so their longest side is at most this many pixels. */
        const val MAX_SIDE = 4096
    }
}
