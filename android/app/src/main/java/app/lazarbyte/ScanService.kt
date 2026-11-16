package app.lazarbyte

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import androidx.core.app.NotificationCompat
import uniffi.lazarbyte_ffi.SourceHandle

/**
 * The foreground service a long scan runs in (AGENTS.md §5): the user can
 * leave the app, and Android will not kill the scan outright while it shows a
 * notification. The scan itself is one blocking bridge call on a worker
 * thread; progress and results flow through [ScanRepository], and the user
 * cancels through it too.
 *
 * The image descriptor is handed over as an extra: the activity detached it
 * from its ParcelFileDescriptor, and [SourceHandle.fromFd] takes ownership on
 * the Rust side (see fdsource.rs for that contract).
 */
class ScanService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    /**
     * Android 14+ gives a dataSync foreground service about six hours per
     * day, then calls this and expects the service to stop within seconds --
     * anything else is a crash. A scan this long is beyond the MVP's honest
     * scope; stopping gracefully leaves what was carved on disk, and the
     * manifest records the run as it was.
     */

    /** The one-argument form Android 14 and 15 call; same graceful stop. */
    override fun onTimeout(startId: Int) {
        ScanRepository.requestCancel()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onTimeout(startId: Int, fgsType: Int) {
        ScanRepository.requestCancel()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onStartCommand(intent: Intent, flags: Int, startId: Int): Int {
        val fd = intent.getIntExtra(EXTRA_FD, -1)
        val outDir = intent.getStringExtra(EXTRA_OUT_DIR) ?: filesDir.resolve("scans/default").absolutePath
        val deep = intent.getBooleanExtra(EXTRA_DEEP, false)
        if (fd < 0) {
            stopSelf()
            return START_NOT_STICKY
        }
        startForeground(NOTIFICATION_ID, notification("Preparing scan"))

        Thread {
            try {
                val source = SourceHandle.fromFd(fd)
                if (deep) {
                    ScanRepository.scanDeep(source, outDir)
                } else {
                    ScanRepository.scanQuick(source, outDir)
                }
            } catch (e: Exception) {
                ScanRepository.state.value = ScanState.Failed(e.message ?: e.javaClass.simpleName)
            } finally {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
        }.start()
        return START_NOT_STICKY
    }

    private fun notification(text: String): Notification {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "Scans", NotificationManager.IMPORTANCE_LOW)
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle("Lazarbyte")
            .setContentText(text)
            .setOngoing(true)
            .build()
    }

    companion object {
        const val CHANNEL_ID = "scans"
        const val NOTIFICATION_ID = 1
        const val EXTRA_FD = "fd"
        const val EXTRA_OUT_DIR = "out_dir"
        const val EXTRA_DEEP = "deep"

        fun start(context: Context, fd: Int, outDir: String, deep: Boolean) {
            val intent = Intent(context, ScanService::class.java)
                .putExtra(EXTRA_FD, fd)
                .putExtra(EXTRA_OUT_DIR, outDir)
                .putExtra(EXTRA_DEEP, deep)
            context.startForegroundService(intent)
        }
    }
}
