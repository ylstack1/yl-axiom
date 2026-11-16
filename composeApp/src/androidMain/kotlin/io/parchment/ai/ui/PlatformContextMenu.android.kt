package io.parchment.ai.ui

import androidx.compose.runtime.Composable

@Composable
internal actual fun PlatformContextMenu(items: () -> List<PlatformMenuItem>, content: @Composable () -> Unit) = content()
