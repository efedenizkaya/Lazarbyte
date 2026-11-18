package app.lazarbyte

import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
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
    private val NOTIFICATIONS_REQUEST_CODE = 41
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

    /** Re-verification results: recovered path -> the hash matched. */
    private var verified by mutableStateOf<Map<String, Boolean>>(emptyMap())

    /** Content-type identification: recovered path -> what the bytes say. */
    private var identified by mutableStateOf<Map<String, uniffi.lazarbyte_ffi.IdentifyOutcome>>(
        emptyMap()
    )

    /**
     * Exporting a recovered file: the user names a destination through SAF
     * and the file's bytes are copied there. One document per export; the
     * bytes come from the app's private storage, where the scan wrote them.
     */
    private var pendingExport: RecoveredItem? = null
    private val exportFile =
        registerForActivityResult(ActivityResultContracts.CreateDocument("*/*")) { uri: Uri? ->
            val item = pendingExport
            pendingExport = null
            if (uri != null && item != null) {
                exportTo(uri, item)
            }
        }

    /**
     * Chain-of-custody check: hash the recovered bytes again and compare
     * with what the manifest reported. Runs on its own thread -- hashing a
     * large file takes time, and the UI updates when the batch lands.
     */
    private fun reVerify(items: List<RecoveredItem>) {
        Thread {
            val inputs = items.map {
                uniffi.lazarbyte_ffi.VerifyInput(path = it.path, sha256 = it.sha256)
            }
            val outcomes = uniffi.lazarbyte_ffi.verifyRecovered(inputs)
            verified = outcomes.associate { it.path to it.ok }
        }.start()
    }

    /**
     * Content-type identification over the accumulated results: what the
     * leading bytes say versus what the recovered name claims. A row whose
     * bytes match no signature reports that, rather than being guessed.
     */
    private fun reIdentify(items: List<RecoveredItem>) {
        Thread {
            val outcomes = uniffi.lazarbyte_ffi.identifyFiles(items.map { it.path })
            identified = outcomes.associateBy { it.path }
        }.start()
    }

    private fun exportTo(uri: Uri, item: RecoveredItem) {
        runCatching {
            contentResolver.openOutputStream(uri, "w")?.use { out ->
                File(item.path).inputStream().use { it.copyTo(out) }
            } ?: throw IllegalStateException("destination not writable")
        }.onFailure {
            sourceError = "Export failed: ${it.message ?: it.javaClass.simpleName}"
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Load the watch list before the UI reads it, and resume the capture
        // service when folders are watched.
        WatchRepository.load(this)
        CaptureService.startIfWatched(this)
        askForNotificationsOnce()
        setContent {
            MaterialTheme {
                var screen by androidx.compose.runtime.remember {
                    androidx.compose.runtime.mutableStateOf("scan")
                }
                Column(modifier = Modifier.fillMaxSize()) {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        OutlinedButton(
                            onClick = { screen = "scan" },
                            modifier = Modifier.weight(1f),
                        ) {
                            Text(if (screen == "scan") "● Scan" else "Scan")
                        }
                        OutlinedButton(
                            onClick = { screen = "trash" },
                            modifier = Modifier.weight(1f),
                        ) {
                            Text(if (screen == "trash") "● Trash" else "Trash")
                        }
                        OutlinedButton(
                            onClick = { screen = "watch" },
                            modifier = Modifier.weight(1f),
                        ) {
                            Text(if (screen == "watch") "● Watch" else "Watch")
                        }
                    }
                    if (screen == "scan") {
                        SourceScreen()
                    } else if (screen == "trash") {
                        TrashScreen()
                    } else {
                        WatchScreen()
                    }
                }
            }
        }
    }

    /**
     * Android 13+ suppresses every notification until this runtime permission
     * is granted -- including the foreground-service notifications that tell
     * the user a scan or a watch is running. Asking once at startup keeps the
     * services visible; a denial does not stop them, only hides the badges.
     */
    private fun askForNotificationsOnce() {
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(
                arrayOf(android.Manifest.permission.POST_NOTIFICATIONS),
                NOTIFICATIONS_REQUEST_CODE,
            )
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
            val handle = try {
                SourceHandle.fromFd(fd)
            } catch (bridgeError: Exception) {
                // fromFd owns the descriptor the moment it is constructed, so
                // even a failure leaves the number in Rust's hands; the
                // duplicate meant for the service is still ours, close it.
                servicePfd.close()
                throw bridgeError
            }
            source = handle
            pendingScanFd = servicePfd
            pickedName = uri.lastPathSegment
            // A superblock peek: which filesystem family the image looks like,
            // before any scan starts. Pure reading, a few kilobytes.
            val probe = handle.probe()
            val family = probe.fatKind ?: probe.extKind ?: "no filesystem at offset 0"
            sourceInfo = "Image open: ${lastPathSegment(uri)}, ${handle.size()} bytes -- " +
                "looks like $family."
        } catch (e: Exception) {
            sourceError = e.message ?: e.javaClass.simpleName
            return
        }
    }

    /** Starts a scan; the service runs it in the foreground. */
    private fun startScan(deep: Boolean) {
        val pfd = pendingScanFd
        if (pfd == null) {
            // The duplicate descriptor is spent after one scan; say so rather
            // than letting the button click silently do nothing.
            sourceError = "Pick the image again to scan it another time."
            return
        }
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
                    androidx.compose.foundation.layout.Box(modifier = Modifier.fillMaxSize()) {
                        // The summary covers the accumulated results, not just
                        // this scan, so quick and deep scans reconcile here.
                        ResultsScreen(
                            items = ScanRepository.results.value,
                            onExport = { item ->
                                pendingExport = item
                                exportFile.launch(item.name)
                            },
                            onClear = {
                                ScanRepository.reset()
                                verified = emptyMap()
                                identified = emptyMap()
                            },
                            onVerifyAll = { reVerify(ScanRepository.results.value) },
                            verified = verified,
                            onIdentifyAll = { reIdentify(ScanRepository.results.value) },
                            identified = identified,
                        )
                    }
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
                enabled = pendingScanFd != null && state !is ScanState.Running,
                onClick = { startScan(deep = false) },
            ) {
                Text("Quick scan (FAT/exFAT)")
            }
            OutlinedButton(
                enabled = pendingScanFd != null && state !is ScanState.Running,
                onClick = { startScan(deep = true) },
            ) {
                Text("Deep scan (carve)")
            }
        }
    }
}
