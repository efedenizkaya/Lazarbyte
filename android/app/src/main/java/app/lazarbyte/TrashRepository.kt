package app.lazarbyte

import android.app.PendingIntent
import android.content.ContentUris
import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.MediaStore

/**
 * The device's shared trash, Android 11 and up (AGENTS.md roadmap, Faz 7).
 *
 * MediaStore marks trashed media with [MediaStore.MediaColumns.IS_TRASHED];
 * the files sit in their original directories as `.trashed-<ts>-<name>` and
 * the system purges them after roughly 30 days -- [MediaStore.MediaColumns.DATE_EXPIRES]
 * carries that deadline. Restoring is `createTrashRequest(..., false)`, which
 * always shows the user a consent dialog, because this app did not trash the
 * files itself. A dialog the user declines is reported as such, not hidden.
 */
data class TrashedItem(
    val uri: Uri,
    val name: String,
    val size: Long,
    val relativePath: String,
    /** When the system will purge the file, epoch seconds; 0 if unknown. */
    val expires: Long,
    /** createTrashRequest takes media URIs only; a Files-view row (a
     * non-media file of some other app) can be listed but not restored. */
    val restorable: Boolean,
)

object TrashRepository {
    /** Trash exists only from Android 11. */
    fun supported(): Boolean = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R

    /**
     * Trashed media across all collections. The synthetic Files view did not
     * return trashed rows on the test device, so each well-defined collection
     * is queried by itself; the restore request takes the item URIs that
     * collection handed out. Needs the media-read permissions granted; a
     * device that hides the trash columns yields an empty list, not a crash.
     */
    fun query(context: Context): List<TrashedItem> {
        if (!supported()) return emptyList()
        val out = mutableListOf<TrashedItem>()
        // Media collections first: the dedupe keeps the first URI seen, and
        // createTrashRequest rejects anything that is not a media item --
        // a Files-view URI crashes the restore ("All requested items must be
        // Media items", verified on the emulator).
        val collections = listOf(
            MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL),
            MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL),
            MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL),
            MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
            MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
        )
        val projection = arrayOf(
            MediaStore.MediaColumns._ID,
            MediaStore.MediaColumns.DISPLAY_NAME,
            MediaStore.MediaColumns.SIZE,
            MediaStore.MediaColumns.RELATIVE_PATH,
            MediaStore.MediaColumns.DATE_EXPIRES,
        )
        // One physical row comes back from several collections (Files and its
        // media views share the same _id), so rows are deduped on identity,
        // preferring the media-collection URI for the restore request.
        val seen = mutableSetOf<Triple<Long, String, String>>()
        // The provider hides trashed rows from every normal query; including
        // them needs the match-trashed query argument set to INCLUDE, not just
        // an is_trashed selection (verified on the emulator). The projection
        // carries IS_TRASHED and the filter is applied client-side: bundling a
        // SQL selection alongside the match arguments proved fragile.
        val args = android.os.Bundle().apply {
            putInt(MediaStore.QUERY_ARG_MATCH_TRASHED, MediaStore.MATCH_INCLUDE)
            putInt(MediaStore.QUERY_ARG_MATCH_PENDING, MediaStore.MATCH_INCLUDE)
        }
        val wideProjection = projection + arrayOf(MediaStore.MediaColumns.IS_TRASHED)
        for (collection in collections) {
            try {
                context.contentResolver.query(
                    collection,
                    wideProjection,
                    args,
                    null,
                )?.use { cursor ->
                    val id = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
                    val name = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.DISPLAY_NAME)
                    val size = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.SIZE)
                    val path = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.RELATIVE_PATH)
                    val expires = cursor.getColumnIndex(MediaStore.MediaColumns.DATE_EXPIRES)
                    val trashed = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.IS_TRASHED)
                    while (cursor.moveToNext()) {
                        if (cursor.isNull(trashed) || cursor.getInt(trashed) != 1) continue
                        val rowKey = Triple(
                            cursor.getLong(id),
                            cursor.getString(name) ?: "",
                            if (cursor.isNull(path)) "" else cursor.getString(path),
                        )
                        if (!seen.add(rowKey)) continue
                        val uri = ContentUris.withAppendedId(collection, cursor.getLong(id))
                        out += TrashedItem(
                            uri = uri,
                            name = cursor.getString(name) ?: "(unnamed)",
                            size = if (cursor.isNull(size)) 0 else cursor.getLong(size),
                            relativePath = if (cursor.isNull(path)) "" else cursor.getString(path),
                            expires = if (expires >= 0 && !cursor.isNull(expires)) cursor.getLong(expires) else 0,
                            restorable = uri.toString().contains("/images/") ||
                                uri.toString().contains("/video/") ||
                                uri.toString().contains("/audio/") ||
                                uri.toString().contains("/downloads/"),
                        )
                    }
                }
            } catch (e: IllegalArgumentException) {
                // This collection lacks the trash columns on this device's
                // provider: skip it rather than pretending.
            } catch (e: SecurityException) {
                // Permissions were revoked mid-session; the UI re-requests.
            }
        }
        return out
    }

    /**
     * Restore pending intent; the user sees the system consent dialog when it
     * is launched. Returns null on pre-11 devices, where the feature does not
     * exist at all.
     */
    fun restoreIntent(context: Context, uris: List<Uri>): PendingIntent? {
        if (!supported() || uris.isEmpty()) return null
        return MediaStore.createTrashRequest(context.contentResolver, uris, false)
    }
}
