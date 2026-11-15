package io.parchment.ai.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.MediaKind
import io.parchment.ai.model.PromptAttachment
import io.parchment.ai.model.WebSourceCitation
import io.parchment.ai.platform.PlatformPickers
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.strings.Strings

/**
 * Attached images and non-image file chips shown below a message body.
 */
@Composable
internal fun TurnAttachmentsSection(
    visible: Boolean,
    attachments: List<PromptAttachment>,
    onInspectAttachment: (PromptAttachment) -> Unit
) {
    if (!visible || attachments.isEmpty()) return
    Spacer(Modifier.height(Space.s8))
    val imageAtts = attachments.filter {
        val m = it.mimeType.lowercase()
        val l = it.fileName.lowercase()
        m.startsWith("image/") || l.endsWith(".png") || l.endsWith(".jpg") ||
            l.endsWith(".jpeg") || l.endsWith(".webp") || l.endsWith(".gif") || l.endsWith(".bmp")
    }
    val otherAtts = attachments.filterNot { it in imageAtts }

    Column(verticalArrangement = Arrangement.spacedBy(Space.s8)) {
        if (imageAtts.isNotEmpty()) {
            @OptIn(ExperimentalLayoutApi::class)
            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(Space.s10),
                verticalArrangement = Arrangement.spacedBy(Space.s10)
            ) {
                imageAtts.forEach { att ->
                    AttachmentImageCard(att, onInspectAttachment)
                }
            }
        }

        if (otherAtts.isNotEmpty()) {
            @OptIn(ExperimentalLayoutApi::class)
            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(Space.s8),
                verticalArrangement = Arrangement.spacedBy(Space.s8)
            ) {
                otherAtts.forEach { att ->
                    AttachmentFileChip(att, onInspectAttachment)
                }
            }
        }
    }
}

@Composable
private fun AttachmentImageCard(
    att: PromptAttachment,
    onInspectAttachment: (PromptAttachment) -> Unit
) {
    val p = LocalParchmentPalette.current
    val fileImage = rememberFileImage(att.localFilePath)
    val bmp = fileImage.bitmap
    Column(
        modifier = Modifier
            .widthIn(max = 360.dp)
            .clip(RoundedCornerShape(Radius.r10))
            .background(p.cardBg)
            .border(LineWidth.hairline, p.border, RoundedCornerShape(Radius.r10))
            .clickable(role = Role.Button, onClickLabel = Strings.openAttachment) { onInspectAttachment(att) }
            .padding(Space.s8),
        verticalArrangement = Arrangement.spacedBy(Space.s6)
    ) {
        if (bmp != null) {
            Image(
                bitmap = bmp,
                contentDescription = att.fileName,
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .heightIn(max = 260.dp)
                    .clip(RoundedCornerShape(Radius.r6))
            )
        } else {
            Box(
                modifier = Modifier
                    .widthIn(min = 200.dp, max = 360.dp)
                    .height(180.dp)
                    .clip(RoundedCornerShape(Radius.r6))
                    .background(p.sheetBg.copy(alpha = 0.6f))
                    .padding(Space.s10),
                contentAlignment = Alignment.Center
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.s6)
                ) {
                    KissVectorIcon(icon = KissIcon.IMAGE, tint = p.accent, size = IconSize.md)
                    BasicText(
                        text = if (fileImage.isLoading) Strings.loading else att.fileName,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        style = p.uiBodyStyle.copy(color = p.accent, fontWeight = FontWeight.SemiBold)
                    )
                }
            }
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.s8)
        ) {
            BasicText(
                text = att.fileName,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = p.uiLabelStyle,
                modifier = Modifier.weight(1f, fill = false)
            )
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.s4)
            ) {
                KissVectorIcon(icon = KissIcon.SEARCH, tint = p.accent, size = IconSize.sm)
                BasicText(
                    text = Strings.zoom,
                    style = p.uiLabelStyle.copy(color = p.accent, fontWeight = FontWeight.SemiBold)
                )
            }
        }
    }
}

@Composable
private fun AttachmentFileChip(
    att: PromptAttachment,
    onInspectAttachment: (PromptAttachment) -> Unit
) {
    val p = LocalParchmentPalette.current
    val mime = att.mimeType.lowercase()
    val lower = att.fileName.lowercase()
    val isPdf = mime == "application/pdf" || lower.endsWith(".pdf")
    val isVid = mime.startsWith("video/") || lower.endsWith(".mp4") ||
        lower.endsWith(".webm") || lower.endsWith(".mov") || lower.endsWith(".mkv")
    val isAud = mime.startsWith("audio/") || lower.endsWith(".mp3") ||
        lower.endsWith(".wav") || lower.endsWith(".ogg") || lower.endsWith(".flac")
    val iconAndAction = when {
        isPdf -> KissIcon.FILE_TEXT to Strings.openPdf
        isVid -> KissIcon.VIDEO to Strings.playVideo
        isAud -> KissIcon.AUDIO to Strings.playAudio
        else -> {
            val stats = if (att.textContent.isNotBlank()) {
                "${att.textContent.lines().size} lines"
            } else Strings.file
            KissIcon.FILE_TEXT to Strings.view(stats)
        }
    }
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(Radius.r8))
            .background(p.cardBg)
            .border(LineWidth.hairline, p.border, RoundedCornerShape(Radius.r8))
            .clickable(role = Role.Button, onClickLabel = Strings.openAttachment) { onInspectAttachment(att) }
            .padding(horizontal = Space.s10, vertical = 7.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(7.dp)
    ) {
        KissVectorIcon(icon = iconAndAction.first, tint = p.accent, size = 14.dp)
        BasicText(
            text = att.fileName,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            style = p.uiBodyStyle.copy(fontWeight = FontWeight.Medium, fontSize = 13.sp),
            modifier = Modifier.widthIn(max = 220.dp)
        )
        BasicText(
            text = "• ${iconAndAction.second}",
            style = p.uiLabelStyle.copy(color = p.accent, fontWeight = FontWeight.SemiBold)
        )
    }
}

/**
 * Inline generated image, video, and audio cards shown within an assistant reply.
 */
@Composable
internal fun TurnGeneratedMediaSection(
    mediaItems: List<GeneratedMediaItem>,
    onInspectGeneratedImage: (GeneratedMediaItem) -> Unit
) {
    if (mediaItems.isEmpty()) return
    Spacer(Modifier.height(Space.s10))
    Column(verticalArrangement = Arrangement.spacedBy(Space.s10)) {
        mediaItems.forEach { media ->
            when (media.kind) {
                MediaKind.IMAGE -> GeneratedImageMediaCard(media, onInspectGeneratedImage)
                MediaKind.VIDEO -> PlayableGeneratedMediaCard(
                    icon = KissIcon.VIDEO,
                    title = Strings.generatedVideo(
                        media.mimeType,
                        if (media.aspectRatio.isNotBlank()) " • ${media.aspectRatio}" else ""
                    ),
                    playButtonText = Strings.playVideoMp4,
                    playTarget = media.localFilePath.ifBlank { media.remoteUrl }
                )
                MediaKind.AUDIO -> PlayableGeneratedMediaCard(
                    icon = KissIcon.AUDIO,
                    title = Strings.generatedAudio(media.mimeType),
                    playButtonText = Strings.playAudioMp3,
                    playTarget = media.localFilePath.ifBlank { media.remoteUrl }
                )
            }
        }
    }
}

private fun parseMediaAspectRatio(raw: String): Float {
    val cleaned = raw.trim()
    if (cleaned.isEmpty()) return 4f / 3f
    val sep = when {
        ':' in cleaned -> ':'
        '/' in cleaned -> '/'
        'x' in cleaned -> 'x'
        'X' in cleaned -> 'X'
        else -> null
    }
    if (sep != null) {
        val parts = cleaned.split(sep, limit = 2)
        val w = parts.getOrNull(0)?.trim()?.toFloatOrNull()
        val h = parts.getOrNull(1)?.trim()?.toFloatOrNull()
        if (w != null && h != null && w > 0f && h > 0f) {
            return (w / h).coerceIn(0.55f, 2.2f)
        }
    }
    val numeric = cleaned.toFloatOrNull()
    if (numeric != null && numeric > 0f) {
        return numeric.coerceIn(0.55f, 2.2f)
    }
    return 4f / 3f
}

@Composable
private fun GeneratedImageMediaCard(
    media: GeneratedMediaItem,
    onInspectGeneratedImage: (GeneratedMediaItem) -> Unit
) {
    val p = LocalParchmentPalette.current
    val fileImage = rememberFileImage(media.localFilePath)
    val bmp = fileImage.bitmap
    val reservedAspect = remember(media.aspectRatio) { parseMediaAspectRatio(media.aspectRatio) }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Radius.r10))
            .background(p.cardBg)
            .border(LineWidth.hairline, p.border, RoundedCornerShape(Radius.r10))
            .padding(Space.s10),
        verticalArrangement = Arrangement.spacedBy(Space.s8)
    ) {
        if (bmp != null) {
            Image(
                bitmap = bmp,
                contentDescription = media.revisedPrompt.ifBlank { "Generated Image" },
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = 520.dp)
                    .clip(RoundedCornerShape(Radius.r6))
                    .clickable(role = Role.Button) {
                        onInspectGeneratedImage(media)
                    }
            )
        } else {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 160.dp, max = 360.dp)
                    .aspectRatio(reservedAspect, matchHeightConstraintsFirst = false)
                    .clip(RoundedCornerShape(Radius.r6))
                    .background(p.sheetBg.copy(alpha = 0.6f))
                    .padding(Space.s12),
                contentAlignment = Alignment.Center
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.s6)
                ) {
                    KissVectorIcon(icon = KissIcon.IMAGE, tint = p.accent, size = IconSize.md)
                    BasicText(
                        text = Strings.generatedImage(media.localFilePath.ifBlank { media.remoteUrl }),
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        style = p.uiBodyStyle.copy(color = p.accent)
                    )
                }
            }
        }
        @OptIn(ExperimentalLayoutApi::class)
        FlowRow(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalArrangement = Arrangement.spacedBy(Space.s6)
        ) {
            BasicText(
                text = "${media.mimeType}${if (media.aspectRatio.isNotBlank()) " • ${media.aspectRatio}" else ""}",
                style = p.uiLabelStyle,
                modifier = Modifier.padding(top = Space.s4)
            )
            Row(horizontalArrangement = Arrangement.spacedBy(Space.s6)) {
                if (bmp != null) {
                    SepiaButton(
                        text = Strings.viewZoom,
                        compact = true,
                        style = SepiaButtonStyle.PRIMARY,
                        onClick = { onInspectGeneratedImage(media) }
                    )
                }
                val openTarget = media.localFilePath.ifBlank { media.remoteUrl }
                if (openTarget.isNotBlank()) {
                    SepiaButton(
                        text = Strings.openInOs,
                        compact = true,
                        style = SepiaButtonStyle.OUTLINE,
                        onClick = { PlatformStorage.openOrPlayMedia(openTarget) }
                    )
                }
                if (media.localFilePath.isNotBlank()) {
                    SepiaButton(
                        text = Strings.copyImage,
                        compact = true,
                        style = SepiaButtonStyle.OUTLINE,
                        onClick = {
                            val bytes = PlatformStorage.readBinaryFileBytes(media.localFilePath)
                            if (bytes != null) {
                                PlatformPickers.copyImageToClipboard(bytes)
                            }
                        }
                    )
                }
            }
        }
    }
}

@Composable
private fun PlayableGeneratedMediaCard(
    icon: KissIcon,
    title: String,
    playButtonText: String,
    playTarget: String
) {
    val p = LocalParchmentPalette.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Radius.r10))
            .background(p.cardBg)
            .border(LineWidth.hairline, p.accentSecondary, RoundedCornerShape(Radius.r10))
            .padding(Space.s12),
        verticalArrangement = Arrangement.spacedBy(Space.s8)
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.s6)
        ) {
            KissVectorIcon(icon = icon, tint = p.accent, size = 15.dp)
            BasicText(
                text = title,
                style = p.uiHeaderStyle.copy(fontSize = 15.sp, color = p.accent)
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(Space.s8)) {
            if (playTarget.isNotBlank()) {
                SepiaButton(
                    text = playButtonText,
                    compact = true,
                    style = SepiaButtonStyle.PRIMARY,
                    onClick = { PlatformStorage.openOrPlayMedia(playTarget) }
                )
            }
        }
    }
}

/**
 * Grounded web sources drawer toggled from the Sources chip in the footer.
 */
@Composable
internal fun TurnWebSourcesDrawer(
    visible: Boolean,
    webSources: List<WebSourceCitation>,
    highlightedSourceLineText: String,
    onHighlightLineChange: (String) -> Unit,
    onClose: () -> Unit
) {
    val p = LocalParchmentPalette.current
    AnimatedVisibility(visible = visible && webSources.isNotEmpty()) {
        Column(
            modifier = Modifier
                .padding(top = Space.s8)
                .fillMaxWidth()
                .clip(RoundedCornerShape(Radius.r10))
                .background(p.sheetBg.copy(alpha = 0.85f))
                .border(LineWidth.hairline, p.accent.copy(alpha = 0.65f), RoundedCornerShape(Radius.r10))
                .padding(11.dp),
            verticalArrangement = Arrangement.spacedBy(Space.s8)
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.s6),
                    modifier = Modifier.weight(1f)
                ) {
                    KissVectorIcon(icon = KissIcon.GLOBE, tint = p.accent, size = IconSize.md)
                    BasicText(
                        text = Strings.groundedWebSourcesTapASource(webSources.size),
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        style = p.uiLabelStyle.copy(color = p.accent, fontWeight = FontWeight.Bold, fontSize = 11.sp)
                    )
                }
                SepiaButton(
                    text = "×",
                    contentDescription = Strings.closeSources,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onClose
                )
            }

            SepiaDivider()

            Column(verticalArrangement = Arrangement.spacedBy(Space.s6)) {
                webSources.forEach { src ->
                    WebSourceCitationCard(
                        src = src,
                        highlightedSourceLineText = highlightedSourceLineText,
                        onHighlightLineChange = onHighlightLineChange
                    )
                }
            }
        }
    }
}

@Composable
private fun WebSourceCitationCard(
    src: WebSourceCitation,
    highlightedSourceLineText: String,
    onHighlightLineChange: (String) -> Unit
) {
    val p = LocalParchmentPalette.current
    val firstGroundedLine = src.groundedLines.firstOrNull().orEmpty()
    val isSelectedLine = highlightedSourceLineText.isNotBlank() &&
        highlightedSourceLineText == firstGroundedLine
    val lineBadgeText = if (src.groundedLineNumbers.isNotEmpty()) {
        if (src.groundedLineNumbers.size == 1) {
            Strings.line(src.groundedLineNumbers.first())
        } else {
            Strings.lines2(src.groundedLineNumbers.joinToString(", "))
        }
    } else {
        Strings.grounded
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Radius.r8))
            .background(if (isSelectedLine) p.accent.copy(alpha = 0.12f) else p.cardBg)
            .border(
                width = if (isSelectedLine) 1.dp else LineWidth.hairline,
                color = if (isSelectedLine) p.accent else p.border,
                shape = RoundedCornerShape(Radius.r8)
            )
            .clickable(role = Role.Button) {
                if (firstGroundedLine.isNotBlank()) {
                    onHighlightLineChange(if (isSelectedLine) "" else firstGroundedLine)
                } else {
                    openWebLink(src.url)
                }
            }
            .padding(horizontal = Space.s10, vertical = Space.s8),
        verticalArrangement = Arrangement.spacedBy(Space.s4)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.s6),
                modifier = Modifier.weight(1f).padding(end = Space.s6)
            ) {
                Box(
                    modifier = Modifier
                        .clip(RoundedCornerShape(Radius.r4))
                        .background(p.accent.copy(alpha = 0.18f))
                        .padding(horizontal = 5.dp, vertical = Space.s2)
                ) {
                    BasicText(
                        text = "[${src.index}]",
                        style = p.monoStyle.copy(color = p.accent, fontWeight = FontWeight.Bold, fontSize = 11.sp)
                    )
                }
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(1.dp)
                ) {
                    BasicText(
                        text = src.title.ifBlank { src.domain.ifBlank { src.url } },
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        style = p.uiBodyStyle.copy(fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                    )
                    if (src.domain.isNotBlank()) {
                        BasicText(
                            text = src.domain,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            style = p.uiLabelStyle.copy(color = p.mutedInk, fontSize = 11.sp)
                        )
                    }
                }
            }
            SepiaButton(
                text = "↗",
                contentDescription = Strings.openSource,
                compact = true,
                style = SepiaButtonStyle.OUTLINE,
                onClick = { openWebLink(src.url) }
            )
        }

        if (firstGroundedLine.isNotBlank()) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.s6)
            ) {
                Box(
                    modifier = Modifier
                        .clip(RoundedCornerShape(Radius.r4))
                        .background(if (isSelectedLine) p.accent.copy(alpha = 0.24f) else p.elevatedBg)
                        .padding(horizontal = Space.s6, vertical = Space.s2)
                ) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Space.s4)
                    ) {
                        KissVectorIcon(
                            icon = KissIcon.CORNER_DOWN_RIGHT,
                            tint = p.accent,
                            size = IconSize.xs
                        )
                        BasicText(
                            text = lineBadgeText,
                            style = p.uiLabelStyle.copy(
                                color = p.accent,
                                fontWeight = FontWeight.Bold,
                                fontSize = 11.sp
                            )
                        )
                    }
                }
                BasicText(
                    text = "\"$firstGroundedLine\"",
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    style = p.uiLabelStyle.copy(
                        color = if (isSelectedLine) p.primaryInk else p.narrationInk,
                        fontStyle = FontStyle.Italic,
                        fontSize = 11.sp
                    ),
                    modifier = Modifier.weight(1f)
                )
            }
        }
    }
}

/**
 * Collapsible raw wire request & response preview drawer with copy buttons.
 */
@Composable
internal fun TurnRawWireDrawer(
    visible: Boolean,
    nodeId: String,
    activeVariantIndex: Int,
    rawRequestPreview: String,
    rawResponsePreview: String
) {
    val p = LocalParchmentPalette.current
    @Suppress("DEPRECATION")
    val clipboard = LocalClipboardManager.current
    AnimatedVisibility(visible = visible && rawRequestPreview.isNotBlank()) {
        var copiedRawWire by remember(nodeId, activeVariantIndex) { mutableStateOf("") }
        SelectionContainer {
            Column(
                modifier = Modifier
                    .padding(top = Space.s6)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(Radius.r8))
                    .background(p.sheetBg)
                    .border(LineWidth.hairline, p.border, RoundedCornerShape(Radius.r8))
                    .padding(Space.s10),
                verticalArrangement = Arrangement.spacedBy(Space.s6)
            ) {
                RawWirePreviewBlock(
                    title = Strings.requestSent,
                    rawText = rawRequestPreview,
                    isCopied = copiedRawWire == "req",
                    onCopy = {
                        clipboard.setText(AnnotatedString(rawRequestPreview))
                        copiedRawWire = "req"
                    }
                )
                if (rawResponsePreview.isNotBlank()) {
                    SepiaDivider()
                    RawWirePreviewBlock(
                        title = Strings.latestResponse,
                        rawText = rawResponsePreview,
                        isCopied = copiedRawWire == "resp",
                        onCopy = {
                            clipboard.setText(AnnotatedString(rawResponsePreview))
                            copiedRawWire = "resp"
                        }
                    )
                }
            }
        }
    }
}

@Composable
private fun RawWirePreviewBlock(
    title: String,
    rawText: String,
    isCopied: Boolean,
    onCopy: () -> Unit
) {
    val p = LocalParchmentPalette.current
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically
    ) {
        BasicText(text = title, style = p.uiLabelStyle.copy(fontWeight = FontWeight.Bold))
        SepiaButton(
            text = if (isCopied) "✓" else "📋",
            compact = true,
            style = SepiaButtonStyle.GHOST,
            onClick = onCopy
        )
    }
    BasicText(text = rawText.take(4000), style = p.monoStyle.copy(fontSize = 11.sp))
}
