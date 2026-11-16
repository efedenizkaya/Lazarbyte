package app.lazarbyte

import android.Manifest
import android.app.Activity
import android.os.Build
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date

private val READ_PERMISSIONS = if (Build.VERSION.SDK_INT >= 33) {
    arrayOf(
        Manifest.permission.READ_MEDIA_IMAGES,
        Manifest.permission.READ_MEDIA_VIDEO,
        Manifest.permission.READ_MEDIA_AUDIO,
    )
} else {
    arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
}

/**
 * The device's shared trash (Android 11+). Every restore goes through the
 * system's own consent dialog -- this app did not trash these files, so the
 * dialog is not ours to skip, and a "no" from the user is reported as a "no".
 */
@Composable
fun TrashScreen() {
    val context = LocalContext.current
    val activity = context as? Activity

    var granted by remember { mutableStateOf(false) }
    var items by remember { mutableStateOf<List<TrashedItem>>(emptyList()) }
    var message by remember { mutableStateOf<String?>(null) }
    var reload by remember { mutableStateOf(0) }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { grants -> granted = grants.values.all { it } }

    val restoreLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartIntentSenderForResult(),
    ) { result ->
        if (result.resultCode == Activity.RESULT_OK) {
            message = "Restored. The file is back in its original folder."
        } else {
            message = "Restore not approved; the file stays in the trash."
        }
        reload++
    }

    LaunchedEffect(granted, reload) {
        if (granted) items = TrashRepository.query(context)
    }
    LaunchedEffect(Unit) {
        if (!granted) permissionLauncher.launch(READ_PERMISSIONS)
    }

    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("Device trash", style = MaterialTheme.typography.titleMedium)
        if (!TrashRepository.supported()) {
            Text(
                "The shared trash needs Android 11 or newer; this device is " +
                    "running Android ${Build.VERSION.SDK_INT}. Nothing can be " +
                    "listed or restored here.",
                style = MaterialTheme.typography.bodyMedium,
            )
            return@Column
        }
        if (!granted) {
            Text(
                "Seeing trashed media from other apps needs the media-read " +
                    "permissions. Nothing is sent anywhere; the app only " +
                    "reads the local media index.",
                style = MaterialTheme.typography.bodySmall,
            )
            Button(onClick = { permissionLauncher.launch(READ_PERMISSIONS) }) {
                Text("Grant media access")
            }
            return@Column
        }
        message?.let {
            Text(it, style = MaterialTheme.typography.bodyMedium)
        }
        if (items.isEmpty()) {
            Text(
                "The trash is empty, or its contents are not visible to this app.",
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        LazyColumn(modifier = Modifier.weight(1f)) {
            items(items) { item ->
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(item.name, style = MaterialTheme.typography.bodyLarge)
                        Text(
                            "${humanSize(item.size)} · ${item.relativePath}" +
                                if (item.expires > 0) {
                                    " · purged " +
                                        DateFormat.getDateInstance()
                                            .format(Date(item.expires * 1000))
                                } else "",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    OutlinedButton(
                        enabled = item.restorable,
                        onClick = {
                            TrashRepository.restoreIntent(context, listOf(item.uri))?.let { pi ->
                                restoreLauncher.launch(
                                    androidx.activity.result.IntentSenderRequest.Builder(pi.intentSender).build(),
                                )
                            }
                        },
                    ) {
                        Text(if (item.restorable) "Restore" else "Restore (media only)")
                    }
                }
                HorizontalDivider()
            }
        }
        OutlinedButton(
            onClick = {
                message = null
                reload++
            },
        ) {
            Text("Reload")
        }
        Text(
            "Restore asks the system for consent every time; the dialog is " +
                "Android's, not this app's.",
            style = MaterialTheme.typography.bodySmall,
        )
    }
}
