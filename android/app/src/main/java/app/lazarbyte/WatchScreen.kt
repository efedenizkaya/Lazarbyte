package app.lazarbyte

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp

/**
 * Watched folders: the prevention feature. The text says exactly what it
 * does and does not do -- root-less Android cannot block a deletion, but a
 * continuously captured copy means the deletion finds the file already safe.
 */
@Composable
fun WatchScreen() {
    val context = LocalContext.current
    val watched by WatchRepository.folders
    val lastSync by WatchRepository.lastSync

    val addFolder = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri: Uri? ->
        uri ?: return@rememberLauncherForActivityResult
        WatchRepository.add(context, uri)
        CaptureService.startIfWatched(context)
    }

    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("Watched folders", style = MaterialTheme.typography.titleMedium)
        Text(
            "This cannot stop a deletion -- nothing without root can. What it " +
                "does: every media file under the watched folders is copied " +
                "into this app's private storage as it changes, so when a " +
                "deletion or a trash purge lands, the last copy is already " +
                "safe here. Deleted files stay in the device trash for about " +
                "30 days first; the trash tab can restore those directly.",
            style = MaterialTheme.typography.bodySmall,
        )
        Button(onClick = { addFolder.launch(null) }) {
            Text("Add a folder to watch")
        }
        lastSync?.let {
            Text(it, style = MaterialTheme.typography.bodyMedium)
        }
        if (watched.isEmpty()) {
            Text(
                "No folders watched yet.",
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        LazyColumn(modifier = Modifier.weight(1f)) {
            items(watched) { tree ->
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        tree.substringAfter("primary:", tree),
                        modifier = Modifier.weight(1f),
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    OutlinedButton(onClick = {
                        WatchRepository.remove(context, tree)
                        if (WatchRepository.folders.value.isEmpty()) {
                            CaptureService.stop(context)
                        }
                    }) {
                        Text("Stop watching")
                    }
                }
                HorizontalDivider()
            }
        }
    }
}
