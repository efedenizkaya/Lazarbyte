package app.lazarbyte

import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.lazarbyte_ffi.SourceHandle

/**
 * Faz 6, step 2: the source round trip. The user picks an image through the
 * Storage Access Framework, the app detaches its file descriptor and hands
 * the *number* to Rust, which takes ownership (see fdsource.rs in
 * lazarbyte-ffi for the safety contract that makes this sound).
 *
 * Ownership notes: `detachFd()` transfers the descriptor to this process's
 * bridge, so the ParcelFileDescriptor is never closed here; the Rust
 * SourceHandle owns the descriptor from the hand-off on and closes it when
 * the handle is closed. A handle opened for a previous image is closed
 * before a new one is opened, so at most one descriptor is alive at a time.
 */
class MainActivity : ComponentActivity() {
    private var source: SourceHandle? = null
    private var sourceInfo by mutableStateOf("No image chosen.")
    private var sourceError by mutableStateOf<String?>(null)

    private val openImage =
        registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
            uri ?: return@registerForActivityResult
            openSource(uri)
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                SourceScreen(
                    info = sourceInfo,
                    error = sourceError,
                    onOpen = { openImage.launch(arrayOf("*/*")) },
                )
            }
        }
    }

    private fun openSource(uri: Uri) {
        source?.close()
        source = null
        sourceError = null
        val pfd = contentResolver.openFileDescriptor(uri, "r")
        if (pfd == null) {
            sourceError = "The document could not be opened for reading."
            return
        }
        try {
            // Ownership moves across the bridge here: from this line on the
            // descriptor belongs to Rust, which closes it when the handle is
            // closed or dropped -- including when opening fails, because
            // SourceHandle.fromFd owns the descriptor the moment it is
            // constructed. Kotlin must not touch the number again.
            val fd = pfd.detachFd()
            val handle = SourceHandle.fromFd(fd)
            source = handle
            sourceInfo = "Image open: ${lastPathSegment(uri)}, ${handle.size()} bytes."
        } catch (e: Exception) {
            sourceError = e.message ?: e.javaClass.simpleName
            return
        }
    }

    private fun lastPathSegment(uri: Uri): String {
        return uri.lastPathSegment ?: uri.toString()
    }
}

@Composable
private fun SourceScreen(info: String, error: String?, onOpen: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Lazarbyte", style = MaterialTheme.typography.headlineMedium)
        Text(info, style = MaterialTheme.typography.bodyMedium)
        error?.let {
            Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
        }
        Button(onClick = onOpen) {
            Text("Choose an image…")
        }
        Text(
            "Scanning is not wired up yet (Faz 6, step 2).",
            style = MaterialTheme.typography.bodySmall,
        )
    }
}
