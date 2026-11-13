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
) {
    val extensions = items.map { it.ext }.distinct().sorted()
    var filter by remember { mutableStateOf<String?>(null) }
    val shown = if (filter == null) items else items.filter { it.ext == filter }

    Column(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
        Text(
            "${items.size} files recovered",
            style = MaterialTheme.typography.titleMedium,
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
                ResultRow(item, onExport)
                HorizontalDivider()
            }
        }
        Button(onClick = onClear, modifier = Modifier.padding(top = 8.dp)) {
            Text("Clear results")
        }
    }
}

@Composable
private fun ResultRow(item: RecoveredItem, onExport: (RecoveredItem) -> Unit) {
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

private fun humanSize(bytes: Long): String = when {
    bytes >= 1 shl 30 -> "%.1f GB".format(bytes.toDouble() / (1 shl 30))
    bytes >= 1 shl 20 -> "%.1f MB".format(bytes.toDouble() / (1 shl 20))
    bytes >= 1 shl 10 -> "%.1f KB".format(bytes.toDouble() / (1 shl 10))
    else -> "$bytes B"
}

private val IMAGE_EXTENSIONS = setOf("jpg", "jpeg", "png", "gif", "bmp")
