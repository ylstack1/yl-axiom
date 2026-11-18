package io.parchment.ai.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlin.test.Test
import kotlin.test.assertEquals

class KissMotionZeroRecompositionTest {
    @OptIn(ExperimentalComposeUiApi::class)
    @Test
    fun hoverAndPressAnimateInLayerPhaseWithoutRecomposingCaller() = runBlocking(Dispatchers.Swing) {
        var compositions = 0
        var clicks = 0
        val scene = ImageComposeScene(width = 200, height = 200, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent {
                SideEffect { compositions++ }
                Box(
                    Modifier
                        .size(100.dp)
                        .kissClickable(onClick = { clicks++ })
                )
            }
            Snapshot.sendApplyNotifications()
            scene.render(0L).close()
            assertEquals(1, compositions)

            var nanoTime = 16_000_000L
            fun advanceFrames(count: Int) {
                repeat(count) {
                    Snapshot.sendApplyNotifications()
                    scene.render(nanoTime).close()
                    nanoTime += 16_000_000L
                }
            }

            scene.sendPointerEvent(PointerEventType.Enter, Offset(50f, 50f))
            scene.sendPointerEvent(PointerEventType.Move, Offset(50f, 50f))
            advanceFrames(15)

            scene.sendPointerEvent(PointerEventType.Press, Offset(50f, 50f))
            advanceFrames(10)

            scene.sendPointerEvent(PointerEventType.Release, Offset(50f, 50f))
            advanceFrames(15)

            assertEquals(1, clicks, "Expected click callback to fire once")
            assertEquals(1, compositions, "Hover and press spring animations must not recompose caller scope")
        } finally {
            scene.close()
        }
    }

    @OptIn(ExperimentalComposeUiApi::class)
    @Test
    fun streamingPulseDotAnimatesInDrawLayerWithoutCallerRecomposition() = runBlocking(Dispatchers.Swing) {
        var compositions = 0
        val scene = ImageComposeScene(width = 120, height = 120, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent {
                SideEffect { compositions++ }
                KissStreamingPulseDot()
            }
            for (f in 0 until 20) {
                Snapshot.sendApplyNotifications()
                scene.render(f * 16_000_000L).close()
            }
            assertEquals(1, compositions, "Smooth VSync pulse dot must animate purely in graphicsLayer without caller recomposition")
        } finally {
            scene.close()
        }
    }
}
