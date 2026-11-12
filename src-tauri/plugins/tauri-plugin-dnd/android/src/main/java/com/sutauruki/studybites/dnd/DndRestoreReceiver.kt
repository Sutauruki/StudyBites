package com.sutauruki.studybites.dnd

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class DndRestoreReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        DndSafety.restore(context)
    }
}
