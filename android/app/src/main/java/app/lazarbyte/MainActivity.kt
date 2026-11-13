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
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.lazarbyte_ffi.SourceHandle
import java.io.File

/**
 * Source selection and scan control. The user picks an image through the
 * Storage Access Framework, the descriptor crosses to Rust (ownership moves
 * with it; see fdsource.rs), and the scans run in [ScanService] so they
 * survive the app leaving the foreground. Output goes into this app's
 * private storage -- a real path the core's default sink writes to -- and
 * the user exports individual files from the results screen (the owner
 * chose this architecture for Faz 6's MVP; a SAF sink can come later).
 */
class MainActivity : ComponentActivity() {
    private var source: SourceHandle? = null
    /** The duplicate descriptor waiting for a scan; Rust never sees it until startScan. */
    private var pendingScanFd: android.os.ParcelFileDescriptor? = null
    private var sourceInfo by mutableStateOf("No image chosen.")
    private var sourceError by mutableStateOf<String?>(null)
    private var pickedName by mutableStateOf<String?>(null)

    private val openImage =
        registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
            uri ?: return@registerForActivityResult
            openSource(uri)
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                SourceScreen()
            }
        }
    }

    private fun openSource(uri: Uri) {
        source?.close()
        pendingScanFd?.close()
        source = null
        pendingScanFd = null
        sourceError = null
        val pfd = contentResolver.openFileDescriptor(uri, "r")
        if (pfd == null) {
            sourceError = "The document could not be opened for reading."
            return
        }
        try {
            // Two descriptors leave this method: one is detached and handed
            // to Rust for the display handle (ownership moves with the
            // number; fdsource.rs carries that contract), the other is a
            // duplicate the activity keeps for the scan service to detach
            // later. If opening fails after detach, Rust owns and closes the
            // descriptor -- Kotlin must not touch the number again.
            val servicePfd = pfd.dup()
            val fd = pfd.detachFd()
            val handle = SourceHandle.fromFd(fd)
            source = handle
            pendingScanFd = servicePfd
            pickedName = uri.lastPathSegment
            sourceInfo = "Image open: ${lastPathSegment(uri)}, ${handle.size()} bytes."
        } catch (e: Exception) {
            sourceError = e.message ?: e.javaClass.simpleName
            return
        }
    }

    /** Starts a scan; the service runs it in the foreground. */
    private fun startScan(deep: Boolean) {
        val pfd = pendingScanFd ?: return
        val dir = File(getExternalFilesDir(null), "scans/${System.currentTimeMillis()}")
        dir.mkdirs()
        // Ownership of the duplicate descriptor moves to the service now; the
        // Rust side closes it when the scan's handle is dropped.
        val fd = pfd.detachFd()
        pendingScanFd = null
        ScanService.start(this, fd, dir.absolutePath, deep)
        sourceInfo = "Scan started on $pickedName."
    }

    private fun lastPathSegment(uri: Uri): String {
        return uri.lastPathSegment ?: uri.toString()
    }

    @Composable
    private fun SourceScreen() {
        val state by ScanRepository.state
        Column(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("Lazarbyte", style = MaterialTheme.typography.headlineMedium)
            Text(sourceInfo, style = MaterialTheme.typography.bodyMedium)
            sourceError?.let {
                Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
            }
            Button(onClick = { openImage.launch(arrayOf("*/*")) }) {
                Text("Choose an image…")
            }
            when (val s = state) {
                is ScanState.Running -> {
                    Text("${s.label}: ${s.scanned} / ${s.total} bytes", style = MaterialTheme.typography.bodyMedium)
                    LinearProgressIndicator(
                        progress = {
                            if (s.total > 0) s.scanned.toFloat() / s.total else 0f
                        },
                        modifier = Modifier.padding(top = 8.dp),
                    )
                    OutlinedButton(onClick = { ScanRepository.requestCancel() }) {
                        Text("Cancel scan")
                    }
                }
                is ScanState.Done -> {
                    Text(
                        "${s.items.size} files recovered (results below).",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                is ScanState.Failed -> {
                    Text(
                        "Scan failed: ${s.message}",
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                ScanState.Idle -> {
                    Text(
                        "Pick a FAT/exFAT image for a quick scan, or any image for a carve scan.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
            OutlinedButton(
                enabled = source != null && state !is ScanState.Running,
                onClick = { startScan(deep = false) },
            ) {
                Text("Quick scan (FAT/exFAT)")
            }
            OutlinedButton(
                enabled = source != null && state !is ScanState.Running,
                onClick = { startScan(deep = true) },
            ) {
                Text("Deep scan (carve)")
            }
        }
    }
}
