package dev.farhanlabib.universalclipboard

import android.app.DownloadManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import uniffi.clip_ffi.SharedFile

/** Files copied on another device: saved to Downloads, put on the clipboard. */
object ReceivedFiles {
    /** Shared files may total at most this much (the core's limit). */
    const val MAX_BYTES = 32L * 1024 * 1024

    private const val FOLDER = "Universal Clipboard"
    private const val CHANNEL = "files"
    private const val NOTIFICATION_ID = 2

    /** Saves [files] under Downloads/Universal Clipboard and returns their URIs. */
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
            resolver.openOutputStream(uri)!!.use { it.write(f.data) }
            values.clear()
            values.put(MediaStore.Downloads.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
            uri
        }
    }

    /** Puts [uris] on the clipboard and tells the user where the files are. Main thread. */
    fun announce(context: Context, names: List<String>, uris: List<Uri>) {
        val resolver = context.contentResolver
        val clip = ClipData.newUri(resolver, "Files from your devices", uris.first())
        uris.drop(1).forEach { clip.addItem(resolver, ClipData.Item(it)) }
        context.getSystemService(ClipboardManager::class.java).setPrimaryClip(clip)

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

    /** Reads shared or copied files, refusing more than [MAX_BYTES] before reading. */
    fun read(context: Context, uris: List<Uri>): List<SharedFile> {
        val resolver = context.contentResolver
        val named = uris.map { uri ->
            var name: String? = null
            var size = -1L
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)
                ?.use { c ->
                    if (c.moveToFirst()) {
                        name = c.getString(0)
                        if (!c.isNull(1)) size = c.getLong(1)
                    }
                }
            Triple(uri, name ?: uri.lastPathSegment ?: "file", size)
        }
        require(named.sumOf { maxOf(it.third, 0L) } <= MAX_BYTES) { "files over 32 MB can't be sent yet" }
        var total = 0L
        return named.map { (uri, name, _) ->
            val data = resolver.openInputStream(uri)!!.use { it.readBytes() }
            total += data.size
            require(total <= MAX_BYTES) { "files over 32 MB can't be sent yet" }
            SharedFile(name, data)
        }
    }

    private fun mimeOf(name: String): String =
        MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase())
            ?: "application/octet-stream"
}
