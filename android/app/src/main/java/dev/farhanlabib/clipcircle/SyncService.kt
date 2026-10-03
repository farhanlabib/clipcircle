package dev.farhanlabib.clipcircle

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.IBinder
import android.util.Log

/**
 * Keeps syncing alive while the app is in the background, so clips from other
 * devices keep arriving. Holds a multicast lock for mDNS discovery, and sends
 * what's copied here automatically once [AutoSend] has its permissions.
 */
class SyncService : Service() {
    private var multicastLock: WifiManager.MulticastLock? = null
    private var autoSend: AutoSend? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(
            NOTIFICATION_ID,
            notification(),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
        )
        if (multicastLock == null) {
            val wifi = applicationContext.getSystemService(WifiManager::class.java)
            multicastLock = wifi.createMulticastLock("clipcircle").apply {
                setReferenceCounted(false)
                acquire()
            }
        }
        clipApp.background({ if (!clipApp.node.isRunning()) clipApp.node.start() }) { result ->
            result.onFailure {
                Log.e(TAG, "could not start syncing", it)
                stopSelf()
            }
        }
        // Started again by the app when Send automatically is turned on or off.
        val wantAutoSend = AutoSend.enabled(this) && AutoSend.ready(this)
        if (wantAutoSend && autoSend == null) {
            autoSend = AutoSend(this).also { it.start() }
        } else if (!wantAutoSend) {
            autoSend?.stop()
            autoSend = null
        }
        running = true
        return START_STICKY
    }

    override fun onDestroy() {
        running = false
        autoSend?.stop()
        autoSend = null
        clipApp.worker.execute { clipApp.node.stop() }
        multicastLock?.release()
        multicastLock = null
        super.onDestroy()
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.channel_name), NotificationManager.IMPORTANCE_LOW),
        )
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        val send = PendingIntent.getActivity(
            this, 1, Intent(this, SendActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.notification_text))
            .setContentIntent(open)
            .addAction(
                Notification.Action.Builder(null, getString(R.string.action_send), send).build(),
            )
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val TAG = "SyncService"
        private const val CHANNEL = "sync"
        private const val NOTIFICATION_ID = 1

        @Volatile
        var running = false
            private set

        fun start(context: Context) {
            context.startForegroundService(Intent(context, SyncService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, SyncService::class.java))
        }
    }
}
