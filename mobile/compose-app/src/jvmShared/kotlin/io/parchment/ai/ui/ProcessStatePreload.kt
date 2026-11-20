package io.parchment.ai.ui

/**
 * Reads settings and the open chat ([initialAppStateLoad]) on a background thread while the platform is still
 * building the window or Activity, so that disk work overlaps with toolkit start-up instead of following it.
 * Only plain data is loaded here; the Compose state holder is still created on the UI thread. The load is a
 * synchronized lazy: if the first frame arrives first, it simply waits for the same load.
 */
internal fun preloadProcessStateInBackground() {
    Thread({ runCatching { initialAppStateLoad.value } }, "parchment-state-preload").apply {
        isDaemon = true
        start()
    }
}
