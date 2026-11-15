package com.sutauruki.studybites.dnd

import android.app.Activity
import android.app.AlarmManager
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.provider.Settings
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class EnableArgs {
    var mode: String = "priority"
    var restoreAfterMinutes: Int = 0
}

@TauriPlugin
class DndPlugin(private val activity: Activity) : Plugin(activity) {
    private val notificationManager =
        activity.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    @Command
    fun status(invoke: Invoke) {
        invoke.resolve(statusValue())
    }

    @Command
    fun requestAccess(invoke: Invoke) {
        activity.startActivity(Intent(Settings.ACTION_NOTIFICATION_POLICY_ACCESS_SETTINGS))
        invoke.resolve()
    }

    @Command
    fun enable(invoke: Invoke) {
        val args = invoke.parseArgs(EnableArgs::class.java)
        val target = when (args.mode) {
            "priority" -> NotificationManager.INTERRUPTION_FILTER_PRIORITY
            "alarms" -> NotificationManager.INTERRUPTION_FILTER_ALARMS
            else -> {
                invoke.reject("Choose priority or alarms mode.")
                return
            }
        }
        if (!notificationManager.isNotificationPolicyAccessGranted) {
            invoke.reject("Allow StudyBites in Do Not Disturb access.")
            return
        }

        val preferences = DndSafety.preferences(activity)
        val wasEngaged = preferences.getBoolean(DndSafety.KEY_ENGAGED, false)
        val current = notificationManager.currentInterruptionFilter
        if (wasEngaged && current != preferences.getInt(DndSafety.KEY_SET_FILTER, -1)) {
            DndSafety.clear(activity)
            invoke.resolve(statusValue())
            return
        }

        val previous = if (wasEngaged) {
            preferences.getInt(DndSafety.KEY_PREVIOUS_FILTER, current)
        } else {
            current
        }
        val minutes = args.restoreAfterMinutes.coerceAtLeast(1)
        val restoreAt = System.currentTimeMillis() + minutes * 60_000L
        preferences.edit()
            .putBoolean(DndSafety.KEY_ENGAGED, true)
            .putInt(DndSafety.KEY_PREVIOUS_FILTER, previous)
            .putInt(DndSafety.KEY_SET_FILTER, target)
            .putString(DndSafety.KEY_MODE, args.mode)
            .putLong(DndSafety.KEY_RESTORE_AT, restoreAt)
            .apply()

        DndSafety.schedule(activity, restoreAt)
        try {
            notificationManager.setInterruptionFilter(target)
            invoke.resolve(statusValue())
        } catch (error: SecurityException) {
            DndSafety.restore(activity)
            invoke.reject(error.message ?: "Do Not Disturb access is unavailable.")
        }
    }

    @Command
    fun restore(invoke: Invoke) {
        DndSafety.restore(activity)
        invoke.resolve(statusValue())
    }

    private fun statusValue(): JSObject {
        val preferences = DndSafety.preferences(activity)
        val engaged = preferences.getBoolean(DndSafety.KEY_ENGAGED, false)
        val setFilter = preferences.getInt(DndSafety.KEY_SET_FILTER, -1)
        val active = engaged && notificationManager.currentInterruptionFilter == setFilter
        val result = JSObject()
        result.put("supported", true)
        result.put("granted", notificationManager.isNotificationPolicyAccessGranted)
        result.put("mode", if (active) preferences.getString(DndSafety.KEY_MODE, "off") else "off")
        return result
    }
}

object DndSafety {
    const val KEY_ENGAGED = "engaged"
    const val KEY_PREVIOUS_FILTER = "previous_filter"
    const val KEY_SET_FILTER = "set_filter"
    const val KEY_MODE = "mode"
    const val KEY_RESTORE_AT = "restore_at"
    private const val PREFERENCES = "studybites_dnd"
    private const val REQUEST_CODE = 7194

    fun preferences(context: Context) =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    fun schedule(context: Context, restoreAt: Long) {
        val alarm = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        alarm.setAndAllowWhileIdle(
            AlarmManager.RTC_WAKEUP,
            restoreAt,
            restoreIntent(context),
        )
    }

    fun restore(context: Context) {
        val preferences = preferences(context)
        if (!preferences.getBoolean(KEY_ENGAGED, false)) {
            cancelAlarm(context)
            return
        }
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (!manager.isNotificationPolicyAccessGranted) {
            cancelAlarm(context)
            return
        }
        val setFilter = preferences.getInt(KEY_SET_FILTER, -1)
        if (manager.currentInterruptionFilter == setFilter) {
            try {
                manager.setInterruptionFilter(
                    preferences.getInt(KEY_PREVIOUS_FILTER, NotificationManager.INTERRUPTION_FILTER_ALL),
                )
            } catch (_: SecurityException) {
                cancelAlarm(context)
                return
            }
        }
        clear(context)
    }

    fun clear(context: Context) {
        preferences(context).edit().clear().apply()
        cancelAlarm(context)
    }

    private fun cancelAlarm(context: Context) {
        val alarm = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        alarm.cancel(restoreIntent(context))
    }

    private fun restoreIntent(context: Context): PendingIntent {
        val intent = Intent(context, DndRestoreReceiver::class.java)
        return PendingIntent.getBroadcast(
            context,
            REQUEST_CODE,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }
}
