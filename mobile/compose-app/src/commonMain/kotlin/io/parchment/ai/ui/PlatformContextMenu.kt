package io.parchment.ai.ui

import androidx.compose.runtime.Composable

/** One entry of a right-click menu. */
internal data class PlatformMenuItem(val label: String, val onClick: () -> Unit)

/**
 * A right-click menu over [content] on desktop, styled from the palette; inside text, the items join the text menu
 * (Copy, Select all). On Android this is [content] alone: a long press there belongs to text selection, and every
 * action stays on the visible buttons.
 */
@Composable
internal expect fun PlatformContextMenu(items: () -> List<PlatformMenuItem>, content: @Composable () -> Unit)
