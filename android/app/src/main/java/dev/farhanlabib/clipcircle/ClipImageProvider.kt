package dev.farhanlabib.clipcircle

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.File
import java.io.FileNotFoundException

/**
 * Serves images received from the circle to whichever app pastes them. The
 * clipboard holds a content:// URI, and Android grants the pasting app read
 * access to it.
 */
class ClipImageProvider : ContentProvider() {
    override fun onCreate() = true

    override fun getType(uri: Uri) = "image/png"

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor =
        ParcelFileDescriptor.open(fileFor(uri), ParcelFileDescriptor.MODE_READ_ONLY)

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor {
        val file = fileFor(uri)
        val columns = projection ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        val row = columns.map {
            when (it) {
                OpenableColumns.DISPLAY_NAME -> file.name
                OpenableColumns.SIZE -> file.length()
                else -> null
            }
        }
        return MatrixCursor(columns, 1).apply { addRow(row) }
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?) = 0
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?) = 0

    private fun fileFor(uri: Uri): File {
        val name = uri.lastPathSegment?.takeIf { NAME.matches(it) }
            ?: throw FileNotFoundException(uri.toString())
        val file = File(dir(context!!), name)
        if (!file.isFile) throw FileNotFoundException(uri.toString())
        return file
    }

    companion object {
        private val NAME = Regex("clip-[0-9]+\\.png")
        private const val KEEP = 5

        private fun dir(context: Context) = File(context.cacheDir, "clips")

        /** Saves [png] and returns a URI for the clipboard. Keeps the last few images. */
        fun save(context: Context, png: ByteArray): Uri {
            val dir = dir(context).apply { mkdirs() }
            val file = File(dir, "clip-${System.currentTimeMillis()}.png")
            file.writeBytes(png)
            dir.listFiles()?.filter { NAME.matches(it.name) }
                ?.sortedByDescending { it.name.removePrefix("clip-").removeSuffix(".png").toLong() }
                ?.drop(KEEP)
                ?.forEach { it.delete() }
            return Uri.Builder()
                .scheme("content")
                .authority("${context.packageName}.clips")
                .appendPath(file.name)
                .build()
        }
    }
}
