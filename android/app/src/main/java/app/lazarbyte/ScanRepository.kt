package app.lazarbyte

import uniffi.lazarbyte_ffi.CarveOptions
import uniffi.lazarbyte_ffi.ProgressCallback
import uniffi.lazarbyte_ffi.ScanCancel
import uniffi.lazarbyte_ffi.SourceHandle
import uniffi.lazarbyte_ffi.UndeleteOptions

/** One recovered file, whatever engine found it. */
data class RecoveredItem(
    /** The engine's own name for the format family: jpg, fat, ext4... */
    val kind: String,
    val name: String,
    val ext: String,
    /** Byte offset of the carve, or of the directory entry for undelete. */
    val offset: Long,
    val size: Long,
    val sha256: String,
    /** The declared length was readable inside the volume. */
    val validated: Boolean,
    val deleted: Boolean,
    /** Where the recovered bytes are, inside the app's private storage. */
    val path: String,
    /** The core's confidence wording: verified / failed / high / low. */
    val confidence: String,
)

/** What a scan is doing right now; observed by the activity and the service. */
sealed class ScanState {
    data object Idle : ScanState()
    data class Running(val label: String, val scanned: Long, val total: Long) : ScanState()
    data class Done(val items: List<RecoveredItem>) : ScanState()
    data class Failed(val message: String) : ScanState()
}

/**
 * Holds the scan's state across the activity and the foreground service, and
 * the handle the user cancels with. The scans are blocking Rust calls, so the
 * service runs them on its own thread; progress comes from the bridge's
 * watcher, which calls in from a Rust-owned thread.
 */
object ScanRepository {
    val state = androidx.compose.runtime.mutableStateOf<ScanState>(ScanState.Idle)

    var cancel: ScanCancel? = null
        private set

    fun requestCancel() {
        cancel?.cancel()
    }

    private class Relay(private val label: String) : ProgressCallback {
        override fun onProgress(
            scanned: kotlin.ULong,
            total: kotlin.ULong,
            files: kotlin.ULong,
            bytesOut: kotlin.ULong,
        ) {
            state.value = ScanState.Running(label, scanned.toLong(), total.toLong())
        }
    }

    /** Quick scan: FAT/exFAT undelete, the mobile profile's bread and butter. */
    fun scanQuick(source: SourceHandle, outDir: String) {
        state.value = ScanState.Running("Quick scan (FAT/exFAT)", 0, source.size().toLong())
        cancel = ScanCancel()
        try {
            val opts = UndeleteOptions(
                outDir = outDir,
                offset = 0UL,
                includeLive = false,
                minSize = 0UL,
            )
            val result = uniffi.lazarbyte_ffi.undeleteFat(source, opts, cancel)
            state.value = ScanState.Done(result.records.map { r ->
                RecoveredItem(
                    kind = r.kind, name = r.name, ext = r.ext, offset = r.offset.toLong(),
                    size = r.size.toLong(), sha256 = r.sha256, validated = r.validated,
                    deleted = r.deleted, path = r.path, confidence = r.confidence,
                )
            })
        } catch (e: Exception) {
            state.value = ScanState.Failed(e.message ?: e.javaClass.simpleName)
        }
    }

    /** Deep scan: carve signatures out of the raw bytes, mobile profile. */
    fun scanDeep(source: SourceHandle, outDir: String) {
        val label = "Deep scan (carve)"
        state.value = ScanState.Running(label, 0, source.size().toLong())
        cancel = ScanCancel()
        try {
            val opts = uniffi.lazarbyte_ffi.mobileCarveOptions().copy(outDir = outDir)
            val records = uniffi.lazarbyte_ffi.scanCarve(source, opts, Relay(label), cancel)
            state.value = ScanState.Done(records.map { r ->
                RecoveredItem(
                    kind = r.kind, name = r.path.substringAfterLast('/'), ext = r.ext,
                    offset = r.offset.toLong(), size = r.size.toLong(), sha256 = r.sha256,
                    validated = r.validated, deleted = false, path = r.path,
                    confidence = r.confidence,
                )
            })
        } catch (e: Exception) {
            state.value = ScanState.Failed(e.message ?: e.javaClass.simpleName)
        }
    }
}
