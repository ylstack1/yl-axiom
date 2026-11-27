package io.parchment.ai.platform

import android.accessibilityservice.AccessibilityService
import android.graphics.Bitmap
import android.util.Base64
import io.parchment.ai.ParchmentAccessibilityService
import java.io.ByteArrayOutputStream
import kotlin.math.min
import kotlin.math.roundToInt

private const val MAX_SCREENSHOT_WIDTH = 1080
private const val SCROLL_STEP_PX = 160f
private const val DOUBLE_TAP_GAP_MS = 60L

/** Works only after the user turns on Parchment in Android's Accessibility settings. */
internal object AndroidComputerControl : ComputerControl {
    @Volatile private var scale = 0.0

    @Volatile private var screenWidth = 0

    @Volatile private var screenHeight = 0

    @Volatile private var screenshotWidth = 0

    @Volatile private var screenshotHeight = 0

    private val service: ParchmentAccessibilityService
        get() = checkNotNull(ParchmentAccessibilityService.instance) { "Turn on YL Axiom in Accessibility settings first." }

    override val isAvailable: Boolean get() = ParchmentAccessibilityService.instance != null

    override fun screenshot(): Result<ComputerScreenshot> = runCatching {
        val full = service.captureScreenshot().getOrThrow()
        val factor = min(1.0, MAX_SCREENSHOT_WIDTH.toDouble() / full.width)
        val width = (full.width * factor).roundToInt().coerceAtLeast(1)
        val height = (full.height * factor).roundToInt().coerceAtLeast(1)
        val scaled = Bitmap.createScaledBitmap(full, width, height, true)
        val bytes = ByteArrayOutputStream().also { scaled.compress(Bitmap.CompressFormat.JPEG, 75, it) }.toByteArray()
        screenWidth = full.width
        screenHeight = full.height
        screenshotWidth = width
        screenshotHeight = height
        scale = factor
        if (scaled !== full) scaled.recycle()
        full.recycle()
        ComputerScreenshot(Base64.encodeToString(bytes, Base64.NO_WRAP), width, height)
    }

    private fun mapCoordinate(rawX: Int, rawY: Int): Pair<Float, Float> {
        check(scale > 0.0 && screenWidth > 0 && screenHeight > 0) { "Take a screenshot first." }
        check(rawX in 0 until screenshotWidth && rawY in 0 until screenshotHeight) { "That point is outside the screen." }
        val x = (rawX / scale).toFloat().coerceIn(0f, (screenWidth - 1).coerceAtLeast(0).toFloat())
        val y = (rawY / scale).toFloat().coerceIn(0f, (screenHeight - 1).coerceAtLeast(0).toFloat())
        return x to y
    }

    private fun point(action: ComputerAction): Pair<Float, Float> = mapCoordinate(
        checkNotNull(action.x) { "That action needs a point." },
        checkNotNull(action.y) { "That action needs a point." }
    )

    private fun Boolean.orFail(what: String) = check(this) { "$what did not work." }

    override fun act(action: ComputerAction): Result<String> = runCatching {
        val target = service
        when (action.action) {
            "click" -> point(action).let { (x, y) -> target.press(x, y, 60).orFail("The tap") }
            "double_click" -> point(action).let { (x, y) ->
                target.press(x, y, 60).orFail("The tap")
                Thread.sleep(DOUBLE_TAP_GAP_MS)
                target.press(x, y, 60).orFail("The tap")
            }
            "right_click" -> point(action).let { (x, y) -> target.press(x, y, 700).orFail("The long press") }
            "scroll" -> {
                check(scale > 0.0 && screenWidth > 0 && screenHeight > 0) { "Take a screenshot first." }
                val (x, y) = if (action.x != null || action.y != null) {
                    mapCoordinate(
                        action.x ?: (screenshotWidth / 2),
                        action.y ?: (screenshotHeight / 2)
                    )
                } else {
                    (screenWidth / 2f) to (screenHeight / 2f)
                }
                val endY = (y - action.amount * SCROLL_STEP_PX).coerceIn(1f, (screenHeight - 1).coerceAtLeast(1).toFloat())
                target.swipe(x, y, x, endY).orFail("The swipe")
            }
            "type" -> target.enterText(action.text).orFail("Typing (tap a text field first)")
            "key" -> when (val keyName = action.keys.lowercase()) {
                "enter", "return" -> target.pressEnter().orFail("Enter (tap a text field first)")
                else -> {
                    val global = when (keyName) {
                        "back" -> AccessibilityService.GLOBAL_ACTION_BACK
                        "home" -> AccessibilityService.GLOBAL_ACTION_HOME
                        "recents" -> AccessibilityService.GLOBAL_ACTION_RECENTS
                        "notifications" -> AccessibilityService.GLOBAL_ACTION_NOTIFICATIONS
                        else -> error("On Android use enter, back, home, recents or notifications.")
                    }
                    target.performGlobalAction(global).orFail("That key")
                }
            }
            "move" -> error("A phone has no pointer to move.")
            else -> error("Unknown action.")
        }
        "Done: ${action.action}."
    }
}
