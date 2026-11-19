package io.parchment.ai

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.text.InputType
import android.util.TypedValue
import android.view.Display
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import io.parchment.ai.harness.ComputerUse
import io.parchment.ai.harness.ToolApprovalBroker
import io.parchment.ai.harness.ToolApprovalDecision
import io.parchment.ai.harness.ToolApprovalRequest
import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.runtime.AppForeground
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** Does nothing by itself: it only carries out actions the user approved in a chat. */
class ParchmentAccessibilityService : AccessibilityService() {
    private val mainHandler = Handler(Looper.getMainLooper())
    private var screenshotExecutor: ExecutorService? = null
    private var gestureThread: HandlerThread? = null
    private var gestureHandler: Handler? = null
    private var overlayScope: CoroutineScope? = null
    private var overlayView: View? = null
    private var currentOverlayRequestId: String? = null
    private var dismissedRequestId: String? = null

    override fun onServiceConnected() {
        instance = this
        startApprovalOverlayObserver()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit
    override fun onInterrupt() = Unit

    override fun onUnbind(intent: Intent?): Boolean {
        instance = null
        stopApprovalOverlayObserver()
        return super.onUnbind(intent)
    }

    override fun onDestroy() {
        instance = null
        stopApprovalOverlayObserver()
        synchronized(this) {
            runCatching { screenshotExecutor?.shutdownNow() }
            screenshotExecutor = null
            runCatching { gestureThread?.quitSafely() }
            gestureThread = null
            gestureHandler = null
        }
        super.onDestroy()
    }

    @Synchronized
    private fun obtainScreenshotExecutor(): ExecutorService = screenshotExecutor ?: Executors.newSingleThreadExecutor().also { screenshotExecutor = it }

    @Synchronized
    private fun obtainGestureHandler(): Handler {
        gestureHandler?.let { return it }
        val thread = HandlerThread("ParchmentA11yGesture").apply { start() }
        gestureThread = thread
        return Handler(thread.looper).also { gestureHandler = it }
    }

    private fun startApprovalOverlayObserver() {
        overlayScope?.cancel()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        overlayScope = scope
        scope.launch {
            combine(ToolApprovalBroker.pendingRequests, AppForeground.isAppVisible) { requests, appVisible ->
                if (appVisible) null else requests.firstOrNull()
            }.collect { request ->
                if (request == null) {
                    dismissedRequestId = null
                    dismissOverlayImmediate()
                } else if (request.requestId != dismissedRequestId) {
                    showOrUpdateOverlay(request)
                }
            }
        }
    }

    private fun stopApprovalOverlayObserver() {
        overlayScope?.cancel()
        overlayScope = null
        if (Looper.myLooper() == Looper.getMainLooper()) dismissOverlayImmediate()
        else mainHandler.post { dismissOverlayImmediate() }
    }

    private fun dismissOverlayImmediate() {
        val existing = overlayView ?: return
        overlayView = null
        currentOverlayRequestId = null
        runCatching { (getSystemService(WINDOW_SERVICE) as? WindowManager)?.removeViewImmediate(existing) }
    }

    private fun ensureOverlayDismissedBeforeCapture() {
        if (overlayView == null) return
        if (Looper.myLooper() == Looper.getMainLooper()) {
            dismissOverlayImmediate()
            return
        }
        val latch = CountDownLatch(1)
        mainHandler.post {
            try {
                dismissOverlayImmediate()
            } finally {
                latch.countDown()
            }
        }
        latch.await(1, TimeUnit.SECONDS)
    }

    private fun reconcileOverlayAfterCapture() {
        val restore = Runnable {
            if (overlayScope == null || AppForeground.isAppVisible.value) return@Runnable
            val next = ToolApprovalBroker.pendingRequests.value.firstOrNull()
            if (next != null && next.requestId != dismissedRequestId) {
                showOrUpdateOverlay(next)
            }
        }
        if (Looper.myLooper() == Looper.getMainLooper()) restore.run() else mainHandler.post(restore)
    }

    private fun showOrUpdateOverlay(request: ToolApprovalRequest) {
        if (currentOverlayRequestId == request.requestId && overlayView != null) return
        dismissOverlayImmediate()
        val wm = getSystemService(WINDOW_SERVICE) as? WindowManager ?: return
        val cardWidth = (resources.displayMetrics.widthPixels - dp(24)).coerceAtMost(dp(420)).coerceAtLeast(dp(260))
        val params = WindowManager.LayoutParams(
            cardWidth,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
            PixelFormat.TRANSLUCENT
        ).apply {
            gravity = Gravity.TOP or Gravity.CENTER_HORIZONTAL
            y = dp(48)
            title = "Parchment Approval"
        }
        val view = buildOverlayView(request)
        runCatching {
            wm.addView(view, params)
            overlayView = view
            currentOverlayRequestId = request.requestId
        }
    }

    private fun onOverlayDecision(requestId: String, decision: ToolApprovalDecision) {
        dismissedRequestId = requestId
        dismissOverlayImmediate()
        overlayScope?.launch {
            ToolApprovalBroker.respondPending(requestId, decision)
        }
    }

    private fun dp(value: Int): Int = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, value.toFloat(), resources.displayMetrics).toInt()

    private fun buildOverlayView(request: ToolApprovalRequest): View {
        val paneTitleText = "${Strings.approval2("Parchment")} · ${request.toolName}"
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val pad = dp(14)
            setPadding(pad, pad, pad, pad)
            background = GradientDrawable().apply {
                setColor(0xFFFAF6EE.toInt())
                cornerRadius = dp(12).toFloat()
                setStroke(dp(1).coerceAtLeast(1), 0xFFC9BCA6.toInt())
            }
            elevation = dp(6).toFloat()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                accessibilityPaneTitle = paneTitleText
            }
        }

        val header = TextView(this).apply {
            text = paneTitleText
            setTextColor(0xFF231D16.toInt())
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            setTypeface(typeface, Typeface.BOLD)
        }
        root.addView(header)

        val intentText = TextView(this).apply {
            text = WorkspaceHarness.describeToolCallIntent(request.toolName, request.argumentsJson)
            setTextColor(0xFF231D16.toInt())
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            setPadding(0, dp(6), 0, 0)
        }
        root.addView(intentText)

        val argsScroll = ScrollView(this).apply {
            val lp = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            lp.topMargin = dp(6)
            layoutParams = lp
        }
        val argsView = TextView(this).apply {
            text = request.argumentsJson
            setTextColor(0xFF5C5144.toInt())
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            typeface = Typeface.MONOSPACE
            maxLines = 4
        }
        argsScroll.addView(argsView)
        root.addView(argsScroll)

        root.addView(
            styledButton(Strings.allowOnce, fill = 0xFF7C4A1E.toInt(), ink = 0xFFFFFFFF.toInt(), stroke = 0xFF7C4A1E.toInt()) {
                onOverlayDecision(request.requestId, ToolApprovalDecision.ALLOW_ONCE)
            }
        )
        if (request.toolName != ComputerUse.TOOL_NAME) {
            root.addView(
                styledButton(Strings.allowThisExactActionForThis, fill = 0xFFF2EBDD.toInt(), ink = 0xFF231D16.toInt(), stroke = 0xFFB8A88F.toInt()) {
                    onOverlayDecision(request.requestId, ToolApprovalDecision.ALLOW_EXACT_ACTION_FOR_RUN)
                }
            )
        }
        root.addView(
            styledButton(Strings.deny, fill = 0xFFF2EBDD.toInt(), ink = 0xFF231D16.toInt(), stroke = 0xFFB8A88F.toInt()) {
                onOverlayDecision(request.requestId, ToolApprovalDecision.DENY)
            }
        )
        root.addView(
            styledButton(Strings.stopRun, fill = 0xFFA62626.toInt(), ink = 0xFFFFFFFF.toInt(), stroke = 0xFFA62626.toInt()) {
                onOverlayDecision(request.requestId, ToolApprovalDecision.STOP)
            }
        )
        return root
    }

    private fun styledButton(label: String, fill: Int, ink: Int, stroke: Int, onClick: () -> Unit): TextView = TextView(this).apply {
        text = label
        contentDescription = label
        setTextColor(ink)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
        setTypeface(typeface, Typeface.BOLD)
        gravity = Gravity.CENTER
        minHeight = dp(48)
        val hPad = dp(12)
        val vPad = dp(10)
        setPadding(hPad, vPad, hPad, vPad)
        background = GradientDrawable().apply {
            setColor(fill)
            cornerRadius = dp(8).toFloat()
            setStroke(dp(1).coerceAtLeast(1), stroke)
        }
        isClickable = true
        isFocusable = false
        accessibilityDelegate = object : View.AccessibilityDelegate() {
            override fun onInitializeAccessibilityNodeInfo(host: View, info: AccessibilityNodeInfo) {
                super.onInitializeAccessibilityNodeInfo(host, info)
                info.className = Button::class.java.name
            }
        }
        layoutParams = LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        ).apply {
            topMargin = dp(8)
        }
        setOnClickListener { onClick() }
    }

    fun captureScreenshot(): Result<Bitmap> {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            return Result.failure(IllegalStateException("Screenshots require Android 11 or newer."))
        }
        ensureOverlayDismissedBeforeCapture()
        try {
            val exec = obtainScreenshotExecutor()
            var lastError = 0
            repeat(SCREENSHOT_MAX_ATTEMPTS) { attempt ->
                val done = CountDownLatch(1)
                val image = AtomicReference<Bitmap?>()
                val failureCode = AtomicInteger(0)
                takeScreenshot(
                    Display.DEFAULT_DISPLAY,
                    exec,
                    object : TakeScreenshotCallback {
                        override fun onSuccess(result: ScreenshotResult) {
                            val buffer = result.hardwareBuffer
                            try {
                                image.set(Bitmap.wrapHardwareBuffer(buffer, result.colorSpace)?.copy(Bitmap.Config.ARGB_8888, false))
                            } finally {
                                buffer.close()
                                done.countDown()
                            }
                        }
                        override fun onFailure(errorCode: Int) {
                            failureCode.set(errorCode)
                            done.countDown()
                        }
                    }
                )
                val finished = done.await(TIMEOUT_SECONDS, TimeUnit.SECONDS)
                image.get()?.let { return Result.success(it) }
                lastError = if (!finished) -1 else failureCode.get()
                if (lastError != ERROR_TAKE_SCREENSHOT_INTERVAL_TIME_SHORT || attempt == SCREENSHOT_MAX_ATTEMPTS - 1) {
                    return Result.failure(IllegalStateException(screenshotErrorMessage(lastError)))
                }
                Thread.sleep(SCREENSHOT_RETRY_DELAY_MS)
            }
            return Result.failure(IllegalStateException(screenshotErrorMessage(lastError)))
        } finally {
            reconcileOverlayAfterCapture()
        }
    }

    private fun screenshotErrorMessage(code: Int): String = when (code) {
        -1 -> "Timed out waiting for the screenshot."
        ERROR_TAKE_SCREENSHOT_SECURE_WINDOW -> "This screen blocks screenshots (secure window)."
        else -> "This Android version or screen does not allow screenshots (code $code)."
    }

    private fun perform(gesture: GestureDescription): Boolean {
        val done = CountDownLatch(1)
        val completed = AtomicBoolean(false)
        val started = dispatchGesture(
            gesture,
            object : GestureResultCallback() {
                override fun onCompleted(description: GestureDescription?) {
                    completed.set(true)
                    done.countDown()
                }
                override fun onCancelled(description: GestureDescription?) = done.countDown()
            },
            obtainGestureHandler()
        )
        return started && done.await(TIMEOUT_SECONDS, TimeUnit.SECONDS) && completed.get()
    }

    private fun stroke(path: Path, durationMs: Long) = GestureDescription.Builder().addStroke(GestureDescription.StrokeDescription(path, 0, durationMs)).build()

    fun press(x: Float, y: Float, durationMs: Long): Boolean = perform(stroke(Path().apply { moveTo(x, y) }, durationMs))

    fun swipe(fromX: Float, fromY: Float, toX: Float, toY: Float): Boolean = perform(stroke(Path().apply { moveTo(fromX, fromY); lineTo(toX, toY) }, 300))

    private fun AccessibilityNodeInfo.acceptsTextInput(): Boolean = isEditable || actionList.any { it.id == AccessibilityNodeInfo.ACTION_SET_TEXT }

    private fun findFocusedInput(): AccessibilityNodeInfo? {
        val activeRoot = rootInActiveWindow
        activeRoot?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)?.let { focused ->
            if (focused.acceptsTextInput()) return focused
            findEditableFocusedNode(focused, depth = 0)?.let { return it }
        }
        val interactive = runCatching { windows }.getOrNull().orEmpty()
        var fallbackFocused: AccessibilityNodeInfo? = null
        for (window in interactive) {
            if (window.type == AccessibilityWindowInfo.TYPE_ACCESSIBILITY_OVERLAY) continue
            val root = window.root ?: continue
            root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)?.let { focused ->
                if (focused.acceptsTextInput()) return focused
                findEditableFocusedNode(focused, depth = 0)?.let { return it }
                if (fallbackFocused == null) fallbackFocused = focused
            }
            findEditableFocusedNode(root, depth = 0)?.let { return it }
        }
        activeRoot?.let { findEditableFocusedNode(it, depth = 0)?.let { editable -> return editable } }
        return fallbackFocused
    }

    private fun findEditableFocusedNode(node: AccessibilityNodeInfo, depth: Int): AccessibilityNodeInfo? {
        if (depth > 24) return null
        if (node.acceptsTextInput() && (node.isFocused || node.isAccessibilityFocused)) return node
        for (i in 0 until node.childCount) {
            val child = node.getChild(i) ?: continue
            findEditableFocusedNode(child, depth + 1)?.let { return it }
        }
        return null
    }

    fun enterText(text: String): Boolean {
        val field = findFocusedInput() ?: return false
        val arguments = Bundle().apply { putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text) }
        return field.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, arguments)
    }

    fun pressEnter(): Boolean {
        val field = findFocusedInput() ?: return false
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
            field.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id)
        ) {
            return true
        }
        if ((field.inputType and InputType.TYPE_TEXT_FLAG_MULTI_LINE) == 0) return false
        val existing = field.text?.toString().orEmpty()
        val arguments = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, "$existing\n")
        }
        return field.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, arguments)
    }

    companion object {
        private const val TIMEOUT_SECONDS = 8L
        private const val SCREENSHOT_MAX_ATTEMPTS = 3
        private const val SCREENSHOT_RETRY_DELAY_MS = 550L

        @Volatile var instance: ParchmentAccessibilityService? = null
            private set
    }
}
