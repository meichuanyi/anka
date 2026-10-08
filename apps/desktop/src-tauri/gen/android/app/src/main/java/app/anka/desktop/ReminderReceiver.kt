package app.anka.desktop

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import java.util.Calendar

/** 每日学习提醒：AlarmManager 每天定时发系统通知。
 *  配置存在 SharedPreferences(anka_prefs)，由 Rust 侧 reminder_set 命令写入；
 *  触发后自我续订明天，BootReceiver 在开机后恢复闹钟。 */
class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        if (!prefs.getBoolean("reminderEnabled", false)) return
        notifyDaily(context)
        scheduleNext(context)
    }

    companion object {
        const val PREFS = "anka_prefs"
        private const val CHANNEL = "daily-reminder"
        private const val REQ_ALARM = 1001
        private const val REQ_TAP = 1002
        private const val NOTIF_ID = 2001

        private fun pendingIntent(context: Context): PendingIntent {
            val i = Intent(context, ReminderReceiver::class.java)
            return PendingIntent.getBroadcast(
                context, REQ_ALARM, i,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }

        fun scheduleNext(context: Context) {
            val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val am = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
            val cal = Calendar.getInstance()
            cal.set(Calendar.HOUR_OF_DAY, prefs.getInt("reminderHour", 20))
            cal.set(Calendar.MINUTE, prefs.getInt("reminderMinute", 30))
            cal.set(Calendar.SECOND, 0)
            cal.set(Calendar.MILLISECOND, 0)
            if (cal.timeInMillis <= System.currentTimeMillis()) cal.add(Calendar.DATE, 1)
            // inexact 闹钟：免 SCHEDULE_EXACT_TOKEN 特殊权限，误差几分钟可接受
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, cal.timeInMillis, pendingIntent(context))
        }

        fun cancel(context: Context) {
            val am = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
            am.cancel(pendingIntent(context))
        }

        private fun notifyDaily(context: Context) {
            val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (Build.VERSION.SDK_INT >= 26) {
                nm.createNotificationChannel(
                    NotificationChannel(CHANNEL, "每日复习提醒", NotificationManager.IMPORTANCE_DEFAULT),
                )
            }
            val tap = Intent(context, MainActivity::class.java)
                .setFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            val pi = PendingIntent.getActivity(
                context, REQ_TAP, tap,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            val b = if (Build.VERSION.SDK_INT >= 26) {
                android.app.Notification.Builder(context, CHANNEL)
            } else {
                @Suppress("DEPRECATION")
                android.app.Notification.Builder(context)
            }
            b.setSmallIcon(context.applicationInfo.icon)
                .setContentTitle("Anka 该背卡啦")
                .setContentText("今天的记忆卡片在等你 · 点击开始复习")
                .setContentIntent(pi)
                .setAutoCancel(true)
            nm.notify(NOTIF_ID, b.build())
        }

        /** Rust reminder_set 命令的入口 */
        @JvmStatic
        fun scheduleOrCancel(context: Context, enabled: Boolean, hour: Int, minute: Int) {
            if (enabled) scheduleNext(context) else cancel(context)
        }
    }
}

/** 开机后恢复已启用的提醒闹钟 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val prefs = context.getSharedPreferences(ReminderReceiver.PREFS, Context.MODE_PRIVATE)
        if (prefs.getBoolean("reminderEnabled", false)) {
            ReminderReceiver.scheduleNext(context)
        }
    }
}
