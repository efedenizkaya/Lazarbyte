package app.lazarbyte

import android.content.ContentUris
import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.MediaStore
import androidx.compose.runtime.mutableStateOf
import java.io.File

/**
 * Watched folders and the capture that protects them.
 *
 * The honest model (AGENTS.md §4): root-less Android cannot stop a deletion,
 * so "prevention" here means the watched folders are continuously backed up
 * into the app's private storage. When a file is deleted or trashed later,
 * the last captured copy already exists, and the sync reports the
 * disappearance instead of pretending it never happened.
 *
 * Watched folders are SAF tree grants with persistable permissions; the sync
 * walks the media index (not the filesystem) because that is what a
 * non-rooted app can see, and copies every media row whose RELATIVE_PATH sits
 * under the watched tree.
 */
object WatchRepository {
    val folders = mutableStateOf<List<String>>(emptyList())
    /** Human-readable summary of the last sync, or null before the first. */
    val lastSync = mutableStateOf<String?>(null)

    private const val PREFS = "watch"
    private const val KEY_FOLDERS = "folders"
    private const val KEY_MANIFEST = "manifest"
    private const val KEY_MANIFEST_PREFIX = "manifest_"

    fun load(context: Context) {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        folders.value = prefs.getStringSet(KEY_FOLDERS, emptySet())?.sorted() ?: emptyList()
    }

    fun add(context: Context, treeUri: Uri) {
        // Persistable grant: the tree stays readable across reboots, which is
        // the whole point of a watch.
        context.contentResolver.takePersistableUriPermission(
            treeUri,
            android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION,
        )
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val set = prefs.getStringSet(KEY_FOLDERS, emptySet())?.toMutableSet() ?: mutableSetOf()
        set += treeUri.toString()
        prefs.edit().putStringSet(KEY_FOLDERS, set).apply()
        load(context)
    }

    fun remove(context: Context, treeUri: String) {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val set = prefs.getStringSet(KEY_FOLDERS, emptySet())?.toMutableSet() ?: mutableSetOf()
        set -= treeUri
        prefs.edit().putStringSet(KEY_FOLDERS, set).apply()
        load(context)
    }

    /** `primary:DCIM/Camera` -- the tree grant -- vs `DCIM/Camera/` in MediaStore. */
    private fun relativePrefix(treeUri: String): String? {
        val docId = DocumentsContract.getTreeDocumentId(Uri.parse(treeUri)) ?: return null
        val path = docId.substringAfter(':', missingDelimiterValue = docId)
        if (path.isEmpty()) return null
        return path.trimEnd('/') + "/"
    }

    private fun capturedDir(context: Context, prefix: String) =
        File(context.filesDir, "captured/${prefix.trim('/').replace('/', '_')}")

    /**
     * One sync pass over every watched folder. Rows still present are copied
     * when new or changed; rows that vanished from the media index keep their
     * last captured copy, and the disappearance is reported.
     */
    fun sync(context: Context): String {
        var copied = 0
        var watched = 0
        var disappeared = 0
        for (tree in folders.value) {
            val prefix = relativePrefix(tree) ?: continue
            val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val manifestKey = KEY_MANIFEST_PREFIX + prefix.hashCode()
            val manifest = prefs.getString(manifestKey, "") ?: ""
            val known: MutableMap<Long, Long> = manifest.split(";")
                .filter { it.isNotBlank() }
                .associate {
                    val parts = it.split(":")
                    (parts[0].toLongOrNull() ?: 0L) to (parts.getOrNull(1)?.toLongOrNull() ?: 0L)
                }
                .toMutableMap()

            val collection = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            val args = android.os.Bundle().apply {
                // Trashed rows are captured too: they are exactly the files a
                // purge would take away next.
                putInt(MediaStore.QUERY_ARG_MATCH_TRASHED, MediaStore.MATCH_INCLUDE)
                putInt(MediaStore.QUERY_ARG_MATCH_PENDING, MediaStore.MATCH_INCLUDE)
            }
            val seenIds = mutableSetOf<Long>()
            context.contentResolver.query(
                collection,
                arrayOf(
                    MediaStore.MediaColumns._ID,
                    MediaStore.MediaColumns.DISPLAY_NAME,
                    MediaStore.MediaColumns.RELATIVE_PATH,
                    MediaStore.MediaColumns.DATE_MODIFIED,
                ),
                args,
                null,
            )?.use { cursor ->
                val id = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
                val name = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.DISPLAY_NAME)
                val path = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.RELATIVE_PATH)
                val modified = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.DATE_MODIFIED)
                while (cursor.moveToNext()) {
                    val rowPath = if (cursor.isNull(path)) "" else cursor.getString(path)
                    if (!rowPath.startsWith(prefix)) continue
                    val rowId = cursor.getLong(id)
                    seenIds += rowId
                    watched++
                    val rowModified = if (cursor.isNull(modified)) 0 else cursor.getLong(modified)
                    if (known[rowId] == rowModified && rowModified != 0L) continue
                    val rowName = cursor.getString(name) ?: continue
                    val dir = capturedDir(context, prefix)
                    dir.mkdirs()
                    val target = java.io.File(dir, rowName)
                    val uri = ContentUris.withAppendedId(collection, rowId)
                    try {
                        context.contentResolver.openInputStream(uri)?.use { input ->
                            target.outputStream().use { output -> input.copyTo(output) }
                        } ?: continue
                        copied++
                        known[rowId] = rowModified
                    } catch (e: Exception) {
                        // One unreadable row must not end the whole sync; the
                        // disappearance check below still runs.
                    }
                }
            }
            for (gone in known.keys - seenIds) {
                // The row vanished; its last captured copy stays on disk.
                known.remove(gone)
                disappeared++
            }
            prefs.edit().putString(
                manifestKey,
                known.entries.joinToString(";") { "${it.key}:${it.value}" },
            ).apply()
        }
        val summary = "$watched watched file(s), $copied captured, $disappeared vanished"
        lastSync.value = summary
        return summary
    }
}
