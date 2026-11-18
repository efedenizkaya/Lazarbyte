package app.lazarbyte

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp

/**
 * What a scan found. Confidence is shown in the core's own words (§4: never
 * "recovered for sure") -- verified/high/low/failed, exactly as the engines
 * report it, with a colour that hints without overclaiming.
 */
@Composable
fun ResultsScreen(
    items: List<RecoveredItem>,
    onExport: (RecoveredItem) -> Unit,
    onClear: () -> Unit,
    onVerifyAll: () -> Unit,
    verified: Map<String, Boolean>,
    onIdentifyAll: () -> Unit,
    identified: Map<String, uniffi.lazarbyte_ffi.IdentifyOutcome>,
) {
    val extensions = items.map { it.ext }.distinct().sorted()
    var filter by remember { mutableStateOf<String?>(null) }
    var search by remember { mutableStateOf("") }
    val searched = if (search.isBlank()) items else items.filter {
        it.name.contains(search.trim(), ignoreCase = true)
    }
    val shown = if (filter == null) searched else searched.filter { it.ext == filter }

    // The summary is pure arithmetic over the rows; the bridge computes it so
    // the definition of "the shape of a result" stays in Rust.
    val summary = remember(items) {
        uniffi.lazarbyte_ffi.summarizeResults(items.map { r ->
            uniffi.lazarbyte_ffi.TriageRowInput(
                kind = r.kind, name = r.name, ext = r.ext,
                size = r.size.toULong(), sha256 = r.sha256, validated = r.validated,
                modified = if (r.modified > 0) r.modified.toULong() else null,
            )
        })
    }
    val duplicates = remember(summary) { summary.duplicateSha256.toSet() }

    Column(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
        Text(
            "${items.size} files recovered",
            style = MaterialTheme.typography.titleMedium,
        )
        Text(
            summary.byKind.joinToString(", ") { "${it.kind}: ${it.files} (${humanSize(it.bytes.toLong())})" },
            style = MaterialTheme.typography.bodySmall,
        )
        val dupNote = if (summary.duplicateRows > 0UL)
            " · ${summary.duplicateRows} byte-identical duplicate(s)" else ""
        val unkNote = if (summary.unknownExtensions > 0UL)
            " · ${summary.unknownExtensions} unknown extension(s)" else ""
        Text(
            "total ${humanSize(summary.totalBytes.toLong())}$dupNote$unkNote",
            style = MaterialTheme.typography.bodySmall,
        )
        summary.modifiedMax?.let { max ->
            val min = summary.modifiedMin ?: max
            Text(
                "modified " + java.text.DateFormat.getDateInstance().format(java.util.Date(min.toLong() * 1000)) +
                    " - " + java.text.DateFormat.getDateInstance().format(java.util.Date(max.toLong() * 1000)),
                style = MaterialTheme.typography.bodySmall,
            )
        }
        Button(onClick = onVerifyAll, modifier = Modifier.padding(top = 4.dp)) {
            Text(if (verified.isEmpty()) "Re-verify recovered files" else "Re-verify again")
        }
        OutlinedButton(
            onClick = onIdentifyAll,
            enabled = items.isNotEmpty(),
            modifier = Modifier.padding(top = 4.dp),
        ) {
            Text(if (identified.isEmpty()) "Identify content types" else "Identify again")
        }
        identified.forEach { (path, outcome) ->
            val name = items.firstOrNull { it.path == path }?.name ?: path
            val line = when {
                outcome.detected == null -> "content unrecognized"
                outcome.matchesExtension -> "type confirmed: ${outcome.detected}"
                else -> "content looks like ${outcome.detected}, name disagrees"
            }
            Text(
                "$name: $line",
                color = when {
                    outcome.matchesExtension -> Color(0xFF2E7D32)
                    outcome.detected == null -> Color(0xFF9E6400)
                    else -> Color(0xFF9E6400)
                },
                style = MaterialTheme.typography.bodySmall,
            )
        }
        verified.forEach { (path, ok) ->
            val name = items.firstOrNull { it.path == path }?.name ?: path
            Text(
                (if (ok) "OK " else "MISMATCH ") + name,
                color = if (ok) Color(0xFF2E7D32) else Color(0xFFC62828),
                style = MaterialTheme.typography.bodySmall,
            )
        }

        OutlinedTextField(
            value = search,
            onValueChange = { search = it },
            label = { Text("Search by name") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        )
        LazyRow(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.padding(vertical = 8.dp),
        ) {
            item {
                AssistChip(onClick = { filter = null }, label = { Text("all") })
            }
            items(extensions) { ext ->
                AssistChip(onClick = { filter = ext }, label = { Text(ext) })
            }
        }
        LazyColumn(modifier = Modifier.weight(1f)) {
            items(shown) { item ->
                ResultRow(item, duplicates, identified[item.path], onExport)
                HorizontalDivider()
            }
        }
        Button(onClick = onClear, modifier = Modifier.padding(top = 8.dp)) {
            Text("Clear results")
        }
    }
}

@Composable
private fun ResultRow(
    item: RecoveredItem,
    duplicates: Set<String>,
    identified: uniffi.lazarbyte_ffi.IdentifyOutcome?,
    onExport: (RecoveredItem) -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (item.ext in IMAGE_EXTENSIONS) {
            Thumbnail(item.path)
        }
        Column(modifier = Modifier.weight(1f)) {
            Text(item.name, style = MaterialTheme.typography.bodyLarge)
            Text(
                "${humanSize(item.size)} · ${item.kind}" +
                    if (item.deleted) " · deleted" else "",
                style = MaterialTheme.typography.bodySmall,
            )
            if (item.sha256 in duplicates) {
                Text(
                    "byte-identical with another result",
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            identified?.let { outcome ->
                val line = when {
                    outcome.detected == null -> "content unrecognized"
                    outcome.matchesExtension -> "content: ${outcome.detected}"
                    else -> "content looks like ${outcome.detected}, name disagrees"
                }
                Text(line, style = MaterialTheme.typography.labelSmall)
            }
            Text(
                text = item.confidence,
                style = MaterialTheme.typography.labelSmall,
                color = confidenceColor(item.confidence),
            )
        }
        Button(onClick = { onExport(item) }) {
            Text("Export")
        }
    }
}

/**
 * A down-scaled preview of a recovered image, decoded from the app's private
 * storage. A file that fails to decode (which happens: carved files can be
 * fragments) shows nothing rather than pretending to be a picture.
 */
@Composable
private fun Thumbnail(path: String) {
    val bitmap by produceState<Bitmap?>(null, path) {
        value = runCatching {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(path, bounds)
            val sample = maxOf(1, maxOf(bounds.outWidth, bounds.outHeight) / 256)
            BitmapFactory.decodeFile(
                path,
                BitmapFactory.Options().apply { inSampleSize = sample },
            )
        }.getOrNull()
    }
    val bmp = bitmap ?: return
    Image(
        bitmap = bmp.asImageBitmap(),
        contentDescription = null,
        contentScale = ContentScale.Crop,
        modifier = Modifier.size(72.dp).padding(0.dp),
    )
}

@Composable
private fun confidenceColor(confidence: String): Color = when (confidence) {
    "verified" -> Color(0xFF2E7D32)
    "high" -> Color(0xFF1565C0)
    "low" -> Color(0xFF9E6400)
    "failed" -> Color(0xFFC62828)
    else -> MaterialTheme.colorScheme.onSurface
}

fun humanSize(bytes: Long): String = when {
    bytes >= 1 shl 30 -> "%.1f GB".format(bytes.toDouble() / (1 shl 30))
    bytes >= 1 shl 20 -> "%.1f MB".format(bytes.toDouble() / (1 shl 20))
    bytes >= 1 shl 10 -> "%.1f KB".format(bytes.toDouble() / (1 shl 10))
    else -> "$bytes B"
}

private val IMAGE_EXTENSIONS = setOf("jpg", "jpeg", "png", "gif", "bmp")
