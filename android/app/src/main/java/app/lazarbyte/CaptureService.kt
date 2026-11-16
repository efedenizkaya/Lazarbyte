package app.lazarbyte

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.database.ContentObserver
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.provider.MediaStore
import androidx.core.app.NotificationCompat
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Watches the media index for changes and keeps the watched folders captured
 * (see WatchRepository for what that honestly means). The observer fires on
 * the provider's threads; the sync runs on its own thread, guarded so two
 * firings never interleave. A change storm during a big copy coalesces into
 * one sync after it, which is the behaviour a backup wants.
 */
class CaptureService : Service() {
    private val syncing = AtomicBoolean(false)
    private val dirty = AtomicBoolean(false)
    private var observer: ContentObserver? = null

    private val worker = Thread {
        while (running) {
            if (dirty.compareAndSet(true, false)) {
                try {
                    WatchRepository.sync(this)
                } catch (e: Exception) {
                    WatchRepository.lastSync.value = "Sync failed: ${e.message}"
                }
            }
            Thread.sleep(500)
        }
    }

    @Volatile
    private var running = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        running = true
        startForeground(NOTIFICATION_ID, notification("Watching folders"))
        observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
            override fun onChange(selfChange: Boolean) {
                dirty.set(true)
            }
        }
        for (uri in OBSERVED_URIS) {
            contentResolver.registerContentObserver(uri, true, observer!!)
        }
        dirty.set(true) // a fresh watch does its first sync immediately
        worker.start()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (WatchRepository.folders.value.isEmpty()) {
            stopSelf()
        }
        return START_STICKY
    }

    /**
     * Android 14+ caps a dataSync foreground service at about six hours per
     * day, then calls this: the service must stop within seconds or the
     * system crashes the app. The watch stops gracefully and says so; the
     * next app launch (or the next folder the user adds) restarts it.
     */

    /** The one-argument form Android 14 and 15 call; same graceful stop. */
    override fun onTimeout(startId: Int) {
        WatchRepository.lastSync.value = "The watch paused: Android ended this " +
            "foreground session. Reopen the app to resume capturing."
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onTimeout(startId: Int, fgsType: Int) {
        WatchRepository.lastSync.value = "The watch paused: Android ended this " +
            "foreground session. Reopen the app to resume capturing."
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        running = false
        observer?.let { contentResolver.unregisterContentObserver(it) }
        super.onDestroy()
    }

    private fun notification(text: String): Notification {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "Folder watch", NotificationManager.IMPORTANCE_LOW)
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle("Lazarbyte")
            .setContentText(text)
            .setOngoing(true)
            .build()
    }

    companion object {
        const val CHANNEL_ID = "watch"
        const val NOTIFICATION_ID = 2

        /** The index that carries RELATIVE_PATH; changes notify descendants. */
        private val OBSERVED_URIS = listOf(
            MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
            MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
            MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
            MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
        )

        /** Starts the watcher when at least one folder is watched. */
        fun startIfWatched(context: Context) {
            if (WatchRepository.folders.value.isEmpty()) return
            context.startForegroundService(Intent(context, CaptureService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, CaptureService::class.java))
        }
    }
}
