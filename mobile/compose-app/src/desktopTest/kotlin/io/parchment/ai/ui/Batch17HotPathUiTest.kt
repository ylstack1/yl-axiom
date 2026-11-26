package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.model.ThemeConfig
import io.parchment.ai.model.ViewMode
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@OptIn(ExperimentalComposeUiApi::class)
class Batch17HotPathUiTest {
    private fun nodes(scene: ImageComposeScene): List<SemanticsNode> {
        fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
        return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
    }

    private fun settle(scene: ImageComposeScene, frameNanos: Long) {
        Snapshot.sendApplyNotifications()
        scene.render(frameNanos).close()
    }

    private fun renderPngBytes(scene: ImageComposeScene, frameNanos: Long): ByteArray {
        Snapshot.sendApplyNotifications()
        return scene.render(frameNanos).use { image -> requireNotNull(image.encodeToData()).bytes }
    }

    private fun click(node: SemanticsNode) {
        assertTrue(requireNotNull(node.config.getOrNull(SemanticsActions.OnClick)?.action).invoke())
    }

    private fun requireNode(scene: ImageComposeScene, label: String, predicate: (SemanticsNode) -> Boolean): SemanticsNode {
        val all = nodes(scene)
        return all.firstOrNull(predicate) ?: error(
            "$label not found among ${all.size} nodes: " + all.mapNotNull { n ->
                val txt = n.config.getOrNull(SemanticsProperties.Text)?.joinToString(",") { it.text }.orEmpty()
                val desc = n.config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(",").orEmpty()
                val set = if (n.config.contains(SemanticsActions.SetText)) "SetText" else ""
                listOf(txt, desc, set).filter { it.isNotBlank() }.joinToString("|").takeIf { it.isNotBlank() }
            }
        )
    }

    @Test
    fun svgPreviewReactsToPaletteInkChange() = runBlocking<Unit>(Dispatchers.Swing) {
        val rawSvg = "<svg viewBox='0 0 100 100'><rect x='10' y='10' width='80' height='80' fill='currentColor'/></svg>"
        val lightPalette = buildPaletteFromConfig(ThemeConfig(primaryInkHex = "#261C14"))
        val accentPalette = buildPaletteFromConfig(ThemeConfig(primaryInkHex = "#00E5FF"))
        val paletteState = mutableStateOf(lightPalette)

        val scene = ImageComposeScene(width = 180, height = 180, density = Density(1f), coroutineContext = Dispatchers.Swing.immediate) {
            CompositionLocalProvider(LocalParchmentPalette provides paletteState.value) {
                Box(Modifier.size(180.dp).background(Color.Black)) {
                    InlineSvgArtifactPreview(rawSvg = rawSvg)
                }
            }
        }
        try {
            val lightBytes = renderPngBytes(scene, 0L)
            paletteState.value = accentPalette
            val darkBytes = renderPngBytes(scene, 16_000_000L)
            assertFalse(
                lightBytes.contentEquals(darkBytes),
                "InlineSvgArtifactPreview must re-parse currentColor against updated LocalYL-AxiomPalette.primaryInk"
            )
        } finally {
            scene.close()
        }
    }

    @Test
    fun lazyMessageEditBufferLoadsFreshContentAfterStreamingAndClearsOnCancel() = runBlocking<Unit>(Dispatchers.Swing) {
        val palette = buildPaletteFromConfig(ThemeConfig())
        val initialNode = MessageNode(
            id = "n-lazy-edit",
            role = MessageRole.ASSISTANT,
            variants = listOf(MessageVariant("v1", "Streaming chunk 1", isStreaming = true)),
            activeVariantIndex = 0,
            includedInContext = false
        )
        val nodeState = mutableStateOf(initialNode)
        val savedEdits = mutableListOf<String>()

        val scene = ImageComposeScene(width = 900, height = 600, density = Density(1f), coroutineContext = Dispatchers.Swing.immediate) {
            CompositionLocalProvider(
                LocalParchmentPalette provides palette,
                LocalParchmentWindowSize provides ParchmentWindowSize(900.dp, 600.dp)
            ) {
                Box(Modifier.fillMaxSize().background(palette.canvasBg).padding(Space.s16)) {
                    MessageTurnItem(
                        node = nodeState.value,
                        nodeIndex = 0,
                        isLatestTurn = true,
                        viewMode = ViewMode.EDITORIAL_TURNS,
                        preferences = PreferencesConfig(),
                        userName = "Reader",
                        characterName = "Assistant",
                        providerName = "Provider",
                        isActivelyThinkingThisNode = false,
                        onSelectVariant = {},
                        onSaveEditedText = { text, _, _ -> savedEdits += text },
                        onToggleContextMute = {},
                        onDeleteNode = {},
                        onForkHere = {},
                        onRegenerate = {},
                        onContinue = {}
                    )
                }
            }
        }
        try {
            settle(scene, 0L)
            assertTrue(nodes(scene).none { it.config.contains(SemanticsActions.SetText) })

            // Simulate streaming deltas while not editing; editor buffer must stay unallocated/unopened.
            nodeState.value = initialNode.copy(
                variants = listOf(MessageVariant("v1", "Streaming chunk 1 -> chunk 2", isStreaming = true))
            )
            settle(scene, 40_000_000L)
            nodeState.value = initialNode.copy(
                variants = listOf(MessageVariant("v1", "Final streamed answer", isStreaming = false))
            )
            settle(scene, 80_000_000L)

            // Hover over turn so desktop action footer fades in via deferred graphicsLayer alpha.
            scene.sendPointerEvent(PointerEventType.Enter, position = Offset(80f, 40f), timeMillis = 100L)
            scene.sendPointerEvent(PointerEventType.Move, position = Offset(80f, 40f), timeMillis = 100L)
            settle(scene, 140_000_000L)
            settle(scene, 450_000_000L)

            val moreBtn = requireNode(scene, "more actions button") { node ->
                node.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(Strings.moreActions) == true
            }
            click(moreBtn)
            settle(scene, 460_000_000L)

            val editBtn = requireNode(scene, "edit button") { node ->
                node.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(Strings.editMessage) == true
            }
            click(editBtn)
            settle(scene, 470_000_000L)

            val editField = requireNode(scene, "edit field") { it.config.contains(SemanticsActions.SetText) }
            assertEquals("Final streamed answer", editField.config.getOrNull(SemanticsProperties.EditableText)?.text)

            val cancelBtn = requireNode(scene, "cancel button") { node ->
                node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == Strings.cancel } == true
            }
            click(cancelBtn)
            settle(scene, 490_000_000L)
            assertTrue(nodes(scene).none { it.config.contains(SemanticsActions.SetText) })

            // Stream/external update followed by reopening editor loads latest text cleanly.
            nodeState.value = initialNode.copy(
                variants = listOf(MessageVariant("v1", "Second updated version", isStreaming = false))
            )
            settle(scene, 510_000_000L)
            val editBtnAfter = requireNode(scene, "edit button after stream") { node ->
                node.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(Strings.editMessage) == true
            }
            click(editBtnAfter)
            settle(scene, 530_000_000L)
            val reopenedField = requireNode(scene, "reopened edit field") { it.config.contains(SemanticsActions.SetText) }
            assertEquals("Second updated version", reopenedField.config.getOrNull(SemanticsProperties.EditableText)?.text)
            assertTrue(savedEdits.isEmpty())
        } finally {
            scene.close()
        }
    }
}
