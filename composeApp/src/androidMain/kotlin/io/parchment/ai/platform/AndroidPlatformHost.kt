package io.parchment.ai.platform

import android.content.Context
import io.parchment.ai.MainActivity
import java.io.File
import java.lang.ref.WeakReference

internal object AndroidPlatformHost {
    @Volatile
    var appContext: Context? = null

    @Volatile
    var mainActivityRef: WeakReference<MainActivity>? = null

    @Volatile
    private var cachedStoragePair: Pair<Context, File>? = null

    fun initAndroid(context: Context, activity: MainActivity) {
        appContext = context.applicationContext
        mainActivityRef = WeakReference(activity)
        // Ensure default local workspace folder exists
        val wsDir = File(resolveStorageDir(), "workspace")
        if (!wsDir.exists()) wsDir.mkdirs()
    }

    fun resolveStorageDir(): File {
        val ctx = appContext
        if (ctx != null) {
            cachedStoragePair?.let { (cachedCtx, cachedDir) ->
                if (cachedCtx === ctx && cachedDir.exists()) return cachedDir
            }
            val dir = ctx.filesDir ?: ctx.noBackupFilesDir ?: File(System.getProperty("java.io.tmpdir", "/data/local/tmp"), "parchment")
            if (!dir.exists()) dir.mkdirs()
            cachedStoragePair = ctx to dir
            return dir
        }
        val dir = File(System.getProperty("java.io.tmpdir", "/data/local/tmp"), "parchment")
        if (!dir.exists()) dir.mkdirs()
        return dir
    }
}
