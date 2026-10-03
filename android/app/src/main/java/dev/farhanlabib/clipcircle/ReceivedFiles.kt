package dev.farhanlabib.clipcircle

import android.app.DownloadManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import java.io.File
import uniffi.clip_ffi.SharedFile

/** Files copied on another device: saved to Downloads, put on the clipboard. */
object ReceivedFiles {
    private const val FOLDER = "ClipCircle"
    private const val CHANNEL = "files"
    private const val NOTIFICATION_ID = 2

    /**
     * Moves [files] (received into app storage) to Downloads/ClipCircle
     * and returns their URIs.
     */
    fun save(context: Context, files: List<SharedFile>): List<Uri> {
        val resolver = context.contentResolver
        return files.map { f ->
            val values = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, f.name)
                put(MediaStore.Downloads.MIME_TYPE, mimeOf(f.name))
                put(MediaStore.Downloads.RELATIVE_PATH, "${Environment.DIRECTORY_DOWNLOADS}/$FOLDER")
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: error("could not save ${f.name}")
            val source = File(f.path)
            resolver.openOutputStream(uri)!!.use { out -> source.inputStream().use { it.copyTo(out) } }
            source.delete()
            values.clear()
            values.put(MediaStore.Downloads.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
            uri
        }
    }

    /** Puts [uris] on the clipboard and tells the user where the files are. Main thread. */
    fun announce(context: Context, names: List<String>, uris: List<Uri>) {
        val resolver = context.contentResolver
        val clip = ClipData.newUri(resolver, ClipApp.LABEL_FILES, uris.first())
        uris.drop(1).forEach { clip.addItem(resolver, ClipData.Item(it)) }
        context.clipApp.setOwnClip(clip)

        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Received files", NotificationManager.IMPORTANCE_DEFAULT),
        )
        val open = if (uris.size == 1) {
            Intent(Intent.ACTION_VIEW)
                .setDataAndType(uris[0], resolver.getType(uris[0]))
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        } else {
            Intent(DownloadManager.ACTION_VIEW_DOWNLOADS)
        }.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val title = if (names.size == 1) "File from your devices" else "${names.size} files from your devices"
        manager.notify(
            NOTIFICATION_ID,
            Notification.Builder(context, CHANNEL)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle(title)
                .setContentText("${names.joinToString(", ")}. Saved in Downloads/$FOLDER and copied.")
                .setContentIntent(PendingIntent.getActivity(context, 2, open, PendingIntent.FLAG_IMMUTABLE))
                .setAutoCancel(true)
                .build(),
        )
    }

    /**
     * Copies shared or copied files into app storage, which the core sends
     * from. The previous batch is deleted first; its sends are long done.
     */
    fun copyForSending(context: Context, uris: List<Uri>): List<SharedFile> {
        val resolver = context.contentResolver
        val dir = File(context.cacheDir, "outgoing")
        dir.deleteRecursively()
        return uris.mapIndexed { i, uri ->
            var name: String? = null
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) name = c.getString(0)
            }
            // One folder per file keeps the copies apart when names repeat.
            val copy = File(File(dir, i.toString()).apply { mkdirs() }, "file")
            resolver.openInputStream(uri)!!.use { input -> copy.outputStream().use { input.copyTo(it) } }
            SharedFile(name ?: uri.lastPathSegment ?: "file", copy.path)
        }
    }

    private fun mimeOf(name: String): String =
        MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase())
            ?: "application/octet-stream"
}
