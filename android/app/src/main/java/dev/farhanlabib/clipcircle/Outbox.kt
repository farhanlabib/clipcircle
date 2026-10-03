package dev.farhanlabib.clipcircle

import android.content.ClipData
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import java.io.ByteArrayOutputStream

/** Sends text, images and files from this phone to the circle. */
object Outbox {
    /** Photos are scaled so their longest side is at most this many pixels. */
    private const val MAX_SIDE = 4096

    /**
     * Sends [clip], read from the clipboard by a screen or window that has
     * focus. Calls [done] on the main thread with a message for the user, or
     * null when there was nothing new to send.
     */
    fun sendClip(context: Context, clip: ClipData?, done: (String?) -> Unit) {
        if (clip == null || clip.itemCount == 0) {
            done(null)
            return
        }
        // Clips that came from the circle go back on the clipboard; don't echo them.
        if (clip.description.label?.toString() in ClipApp.OWN_LABELS) {
            done(null)
            return
        }
        val item = clip.getItemAt(0)
        val uri = item.uri
        when {
            uri != null && clip.description.hasMimeType("image/*") -> sendImage(context, uri, done)
            // Copied files: URIs without text.
            uri != null && item.text == null ->
                sendFiles(context, (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }, done)
            else -> sendText(context, item.coerceToText(context)?.toString(), done)
        }
    }

    fun sendText(context: Context, text: String?, done: (String?) -> Unit) {
        if (text.isNullOrEmpty()) {
            done(null)
            return
        }
        send(context, done) { context.clipApp.node.sendText(text) }
    }

    fun sendImage(context: Context, uri: Uri, done: (String?) -> Unit) =
        send(context, done) { context.clipApp.node.sendImage(readAsPng(context, uri)) }

    fun sendFiles(context: Context, uris: List<Uri>, done: (String?) -> Unit) {
        if (uris.isEmpty()) {
            done(null)
            return
        }
        send(context, done) { context.clipApp.node.sendFiles(ReceivedFiles.copyForSending(context, uris)) }
    }

    private fun send(context: Context, done: (String?) -> Unit, work: () -> Unit) {
        val app = context.clipApp
        app.background({
            if (!app.node.isRunning()) app.node.start()
            work()
        }) { result ->
            done(if (result.isSuccess) "Sent to your devices" else "Could not send: ${result.exceptionOrNull()?.message}")
        }
    }

    /** PNG bytes for the image at [uri], scaled down if it is very large. */
    private fun readAsPng(context: Context, uri: Uri): ByteArray {
        val resolver = context.contentResolver
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        resolver.openInputStream(uri)!!.use { BitmapFactory.decodeStream(it, null, bounds) }
        val longest = maxOf(bounds.outWidth, bounds.outHeight)
        require(longest > 0) { "not an image" }
        if (bounds.outMimeType == "image/png" && longest <= MAX_SIDE) {
            return resolver.openInputStream(uri)!!.use { it.readBytes() }
        }
        var sample = 1
        while (longest / sample > MAX_SIDE) sample *= 2
        val options = BitmapFactory.Options().apply { inSampleSize = sample }
        val bitmap = resolver.openInputStream(uri)!!.use { BitmapFactory.decodeStream(it, null, options) }
            ?: error("not an image")
        return ByteArrayOutputStream().use { out ->
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
            bitmap.recycle()
            out.toByteArray()
        }
    }
}
