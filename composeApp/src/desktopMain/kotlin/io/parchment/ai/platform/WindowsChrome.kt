package io.parchment.ai.platform

/**
 * Themes the real Windows caption, which keeps native animations, Snap Layouts and rounded corners.
 * The caption shows the app icon and the window title (the open chat), drawn in the theme's ink.
 */
internal object WindowsChrome {
    private const val DARK_MODE = 20
    private const val CORNERS = 33
    private const val BORDER_COLOR = 34
    private const val CAPTION_COLOR = 35
    private const val TEXT_COLOR = 36
    private const val ROUND_CORNERS = 2

    private fun colorRef(argb: Int): Int =
        ((argb and 0xFF) shl 16) or (argb and 0xFF00) or ((argb shr 16) and 0xFF)

    /** Returns each call's result code, 0 meaning success. Older Windows builds decline the color attributes. */
    fun applyCaption(handle: Long, captionArgb: Int, inkArgb: Int, borderArgb: Int, dark: Boolean): Map<String, Int> {
        if (!WindowsNative.isWindows || handle == 0L) return emptyMap()
        return runCatching {
            mapOf(
                "darkMode" to WindowsNative.setWindowAttribute(handle, DARK_MODE, if (dark) 1 else 0),
                "corners" to WindowsNative.setWindowAttribute(handle, CORNERS, ROUND_CORNERS),
                "caption" to WindowsNative.setWindowAttribute(handle, CAPTION_COLOR, colorRef(captionArgb)),
                "text" to WindowsNative.setWindowAttribute(handle, TEXT_COLOR, colorRef(inkArgb)),
                "border" to WindowsNative.setWindowAttribute(handle, BORDER_COLOR, colorRef(borderArgb))
            )
        }.getOrDefault(emptyMap())
    }
}
