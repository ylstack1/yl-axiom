package io.parchment.ai.ui

import androidx.compose.foundation.ContextMenuArea
import androidx.compose.foundation.ContextMenuItem
import androidx.compose.foundation.DefaultContextMenuRepresentation
import androidx.compose.foundation.LocalContextMenuRepresentation
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember

@Composable
internal actual fun PlatformContextMenu(items: () -> List<PlatformMenuItem>, content: @Composable () -> Unit) {
    val p = LocalParchmentPalette.current
    val representation = remember(p.elevatedBg, p.primaryInk, p.accent) {
        DefaultContextMenuRepresentation(
            backgroundColor = p.elevatedBg,
            textColor = p.primaryInk,
            itemHoverColor = p.accent.copy(alpha = 0.14f)
        )
    }
    CompositionLocalProvider(LocalContextMenuRepresentation provides representation) {
        ContextMenuArea(items = { items().map { ContextMenuItem(it.label, it.onClick) } }) {
            content()
        }
    }
}
