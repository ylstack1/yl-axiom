package io.parchment.ai.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.model.ModelPricingSpec
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.keepsProviderState
import io.parchment.ai.strings.Strings

/**
 * Compact Circular Context Ring + Token/Cost Pill in the Composer Toolbar.
 * Uses smooth spring animation when token counts change.
 */
@Composable
fun SepiaContextGaugeRing(
    usedTokens: Int,
    contextLimitTokens: Int,
    sessionCostUsd: Double = 0.0,
    onClick: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
    sessionCostLabel: String = ""
) {
    val p = LocalParchmentPalette.current
    val isUnlimited = contextLimitTokens <= 0
    val safeLimit = if (isUnlimited) 1_000_000 else contextLimitTokens
    val targetFraction = if (isUnlimited) 0.08f else (usedTokens.toFloat() / safeLimit.toFloat()).coerceIn(0f, 1f)
    val animatedFractionState = animateFloatAsState(
        targetValue = targetFraction,
        animationSpec = motionSpring(Spring.StiffnessLow)
    )
    val percent = (targetFraction * 100f).toInt()

    val ringColorState = animateColorAsState(
        targetValue = when {
            !isUnlimited && targetFraction >= 0.85f -> p.danger
            !isUnlimited && targetFraction >= 0.60f -> p.accentSecondary
            else -> p.accent
        },
        animationSpec = motionTween(Motion.SLOW)
    )
    val density = androidx.compose.ui.platform.LocalDensity.current
    val ringStroke = remember(density) { Stroke(width = with(density) { LineWidth.focus.toPx() }, cap = StrokeCap.Round) }

    val shape = RoundedCornerShape(Radius.r8)
    val gaugeInteraction = remember { MutableInteractionSource() }
    val gaugeHovered by gaugeInteraction.collectIsHoveredAsState()
    val isCostUnavailable = sessionCostLabel == "cost unavailable"
    val gaugeStateSummary = remember(usedTokens, safeLimit, isUnlimited, isCostUnavailable) {
        val usedStr = formatCompactTokens(usedTokens)
        val limitStr = if (isUnlimited) "∞" else formatCompactTokens(safeLimit)
        val usagePart = Strings.tokensUsedInConversation(usedStr, limitStr)
        if (isCostUnavailable) "$usagePart, ${Strings.costUnavailable}" else usagePart
    }
    Row(
        modifier = modifier
            .heightIn(min = ControlHeight.compact)
            .then(
                if (onClick != null) Modifier.kissClickable(interactionSource = gaugeInteraction, onClick = onClick)
                else Modifier.hoverable(gaugeInteraction)
            )
            .semantics { stateDescription = gaugeStateSummary }
            .clip(shape)
            .background(p.cardBg)
            .border(LineWidth.hairline, p.border, shape)
            .padding(horizontal = Space.s8, vertical = Space.s6),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp)
    ) {
        Box(
            modifier = Modifier.size(16.dp),
            contentAlignment = Alignment.Center
        ) {
            Canvas(modifier = Modifier.size(IconSize.md)) {
                drawArc(
                    color = p.border.copy(alpha = 0.6f),
                    startAngle = 0f,
                    sweepAngle = 360f,
                    useCenter = false,
                    style = ringStroke
                )
                drawArc(
                    color = ringColorState.value,
                    startAngle = -90f,
                    sweepAngle = (animatedFractionState.value * 360f).coerceAtLeast(6f),
                    useCenter = false,
                    style = ringStroke
                )
            }
        }

        val win = LocalParchmentWindowSize.current
        val costTag = if (!win.isCompact && !isCostUnavailable) {
            sessionCostLabel.takeIf { it.isNotBlank() }?.let { " • $it" }
                ?: if (sessionCostUsd > 0.0) " • ~${formatUsdCost(sessionCostUsd)}" else ""
        } else ""
        val labelStr = if (win.isCompact) {
            formatCompactTokens(usedTokens)
        } else if (isUnlimited) {
            "${formatCompactTokens(usedTokens)}/∞$costTag"
        } else {
            "${formatCompactTokens(usedTokens)}/${formatCompactTokens(safeLimit)} ($percent%)$costTag"
        }
        BasicText(
            text = labelStr,
            maxLines = 1,
            softWrap = false,
            overflow = TextOverflow.Ellipsis,
            style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.narrationInk)
        )
        if (!win.isCompact && isCostUnavailable) {
            KissVectorIcon(icon = KissIcon.WARNING, tint = p.accentSecondary, size = IconSize.sm)
        }
        val hintText = if (isCostUnavailable) "${Strings.howMuchOfThisModelS}\n${Strings.costUnavailable}" else Strings.howMuchOfThisModelS
        HoverHint(hintText, gaugeHovered)
    }
}

/**
 * Counts words in one pass without allocating, since it runs on every streamed update.
 */
fun countWordsFast(text: CharSequence): Int {
    var count = 0
    var inWord = false
    for (i in 0 until text.length) {
        if (text[i].isWhitespace()) {
            inWord = false
        } else if (!inWord) {
            inWord = true
            count++
        }
    }
    return count
}

/** Each new frame of a long reply copies its text; keep VSync-locked up to 32 KB and >=60Hz up to 128 KB. */
internal fun visibleTextCopyIntervalMs(length: Int): Long = when {
    length < 32_000 -> 0L
    length < 128_000 -> 16L
    else -> 32L
}

/** Counts like [countWordsFast] but scans only what was appended since the last call. */
internal class StreamWordCounter {
    private var scanned = 0
    private var inWord = false
    private var total = 0

    fun count(text: CharSequence): Int {
        if (text.length < scanned) reset()
        for (i in scanned until text.length) {
            if (text[i].isWhitespace()) {
                inWord = false
            } else if (!inWord) {
                inWord = true
                total++
            }
        }
        scanned = text.length
        return total
    }

    /** Call when the buffer is replaced rather than appended to. */
    fun reset() {
        scanned = 0
        inWord = false
        total = 0
    }
}

fun formatCompactTokens(tokens: Int): String = formatCompactTokens(tokens.toLong())

fun formatCompactTokens(tokens: Long): String = when {
    tokens >= 1_000_000 -> {
        val m = (tokens / 100_000) / 10.0
        if (m == m.toLong().toDouble()) "${m.toLong()}M" else "${m}M"
    }
    tokens >= 10_000 -> "${tokens / 1000}k"
    tokens >= 1_000 -> {
        val k = (tokens / 100) / 10.0
        "${k}k"
    }
    else -> tokens.toString()
}

fun formatUsdCost(costUsd: Double): String {
    if (costUsd <= 0.0) return "$0.00"
    if (costUsd < 0.0001) return "<$0.0001"
    val scaled = (costUsd * 10000.0 + 0.5).toLong()
    val whole = scaled / 10000
    val frac = (scaled % 10000).toString().padStart(4, '0').trimEnd('0').let {
        if (it.length < 2) it.padEnd(2, '0') else it
    }
    return "$$whole.$frac"
}

enum class PrivacyBadgeTone {
    POSITIVE,
    NEUTRAL,
    CAUTION
}

@Immutable
data class ModelPrivacyBadge(
    val label: String,
    val detail: String,
    val tone: PrivacyBadgeTone
)

private fun isLocalEndpointUrl(baseUrl: String): Boolean {
    val trimmed = baseUrl.trim().lowercase()
    if (trimmed.isEmpty()) return false
    val hostPort = trimmed.substringAfter("://", trimmed).substringBefore('/').substringBefore('?')
    val host = if (hostPort.startsWith("[")) {
        hostPort.substringAfter('[').substringBefore(']')
    } else {
        hostPort.substringBefore(':')
    }
    if (host == "localhost" || host == "::1" || host == "0.0.0.0" || host.endsWith(".local")) return true
    if (host.startsWith("127.") || host.startsWith("10.") || host.startsWith("192.168.")) return true
    if (host.startsWith("172.")) {
        val second = host.removePrefix("172.").substringBefore('.').toIntOrNull()
        if (second != null && second in 16..31) return true
    }
    return false
}

/**
 * Derives truthful, local-only privacy & retention badges from endpoint locality, provider storage flags,
 * and catalog pricing metadata (zero extra network requests, zero hardcoded model IDs).
 */
internal fun resolveModelPrivacyBadges(
    profile: ProviderProfile,
    modelId: String = profile.defaultModel,
    spec: ModelPricingSpec? = profile.customModelSpecs[modelId]
): List<ModelPrivacyBadge> = buildList {
    val isLocal = isLocalEndpointUrl(profile.baseUrl)
    if (isLocal) {
        add(ModelPrivacyBadge(Strings.privacyBadgeLocal, Strings.privacyBadgeLocalDetail, PrivacyBadgeTone.POSITIVE))
        return@buildList
    }
    val supportsStorageToggle = profile.protocol == ProviderProtocol.GEMINI_INTERACTIONS ||
        profile.protocol == ProviderProtocol.OPENAI_RESPONSES ||
        profile.keepsProviderState
    if (supportsStorageToggle) {
        if (profile.keepsProviderState) {
            add(ModelPrivacyBadge(Strings.privacyBadgeStored, Strings.privacyBadgeStoredDetail, PrivacyBadgeTone.CAUTION))
        } else {
            add(ModelPrivacyBadge(Strings.privacyBadgeNoStore, Strings.privacyBadgeNoStoreDetail, PrivacyBadgeTone.POSITIVE))
        }
    }
    val kp = spec?.knownPricing
    val isKnownFree = modelId.trim().endsWith(":free", ignoreCase = true) ||
        (kp != null && kp.inputPerM == 0.0 && kp.outputPerM == 0.0)
    val isKnownPaid = !isKnownFree &&
        spec != null &&
        (
            (kp?.inputPerM ?: spec.inputPricePerM) > 0.0 ||
                (kp?.outputPerM ?: spec.outputPricePerM) > 0.0
            )
    when {
        isKnownFree -> add(ModelPrivacyBadge(Strings.privacyBadgeFreeTier, Strings.privacyBadgeFreeTierDetail, PrivacyBadgeTone.CAUTION))
        isKnownPaid -> add(ModelPrivacyBadge(Strings.privacyBadgePaidApi, Strings.privacyBadgePaidApiDetail, PrivacyBadgeTone.NEUTRAL))
    }
}

internal fun primaryModelPrivacyBadge(
    profile: ProviderProfile,
    modelId: String,
    spec: ModelPricingSpec? = profile.customModelSpecs[modelId]
): ModelPrivacyBadge? {
    val badges = resolveModelPrivacyBadges(profile, modelId, spec)
    return badges.firstOrNull { it.tone == PrivacyBadgeTone.CAUTION }
        ?: badges.firstOrNull { it.tone == PrivacyBadgeTone.POSITIVE }
        ?: badges.firstOrNull()
}

@Composable
internal fun ModelPrivacyBadgeChip(
    badge: ModelPrivacyBadge,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val toneColor = when (badge.tone) {
        PrivacyBadgeTone.POSITIVE -> p.success
        PrivacyBadgeTone.CAUTION -> p.danger
        PrivacyBadgeTone.NEUTRAL -> p.mutedInk
    }
    val shape = RoundedCornerShape(Radius.r6)
    Box(
        modifier = modifier
            .clip(shape)
            .background(toneColor.copy(alpha = if (badge.tone == PrivacyBadgeTone.NEUTRAL) 0.10f else 0.13f))
            .border(LineWidth.hairline, toneColor.copy(alpha = 0.45f), shape)
            .padding(horizontal = Space.s6, vertical = Space.s2)
    ) {
        BasicText(
            text = badge.label,
            maxLines = 1,
            style = p.uiLabelStyle.copy(
                fontSize = 10.5.sp,
                fontWeight = FontWeight.Medium,
                color = toneColor
            )
        )
    }
}

data class SearchableModelOption(
    val profileId: String,
    val profileName: String,
    val modelId: String,
    val vendorGroup: String,
    val contextWindowTokens: Int,
    val privacyBadge: ModelPrivacyBadge? = null
)

/**
 * Universal Searchable Vertical Model Selector (`SepiaSearchableModelPicker`):
 * - Search filter input at top (`Enter` immediately selects top match, `Clear` resets).
 * - Provider filter pills (when >1 provider has models) + automatic vendor-family filter pills (`deepseek-ai`, `Qwen`, `zai-org`, `google`, etc.).
 * - Bounded `LazyColumn` vertical list (`heightIn(max = maxListHeight)`) with active model pinned first when unfiltered,
 *   clean monospace model ID on the left, and provider + context window badges on the right.
 */
@Composable
fun SepiaSearchableModelPicker(
    profiles: List<ProviderProfile>,
    selectedProfileId: String,
    selectedModelId: String,
    onSelectModel: (profileId: String, modelId: String) -> Unit,
    modifier: Modifier = Modifier,
    maxListHeight: Dp = 280.dp,
    placeholder: String = Strings.searchModels
) {
    val p = LocalParchmentPalette.current
    var searchQuery by remember { mutableStateOf("") }
    var selectedProviderFilter by remember { mutableStateOf("") } // "" = All providers
    var selectedVendorFilter by remember { mutableStateOf("") } // "" = All vendors

    val allOptions = remember(profiles) {
        profiles.flatMap { prof ->
            val models = if (prof.savedModels.isNotEmpty()) prof.savedModels
            else if (prof.defaultModel.isNotBlank()) listOf(prof.defaultModel)
            else emptyList()
            models.map { m ->
                val vendor = if (m.contains('/')) m.substringBefore('/').trim() else ""
                val spec = prof.customModelSpecs[m]
                val ctx = spec?.contextWindowTokens ?: prof.contextWindowTokens
                SearchableModelOption(
                    profileId = prof.id,
                    profileName = prof.name,
                    modelId = m,
                    vendorGroup = vendor,
                    contextWindowTokens = ctx,
                    privacyBadge = primaryModelPrivacyBadge(prof, m, spec)
                )
            }
        }
    }

    val providerCounts = remember(allOptions) {
        allOptions.groupBy { it.profileId to it.profileName }
            .map { (pair, list) -> Triple(pair.first, pair.second, list.size) }
    }

    val vendorCounts = remember(allOptions, selectedProviderFilter) {
        val scoped = if (selectedProviderFilter.isEmpty()) allOptions else allOptions.filter { it.profileId == selectedProviderFilter }
        scoped.filter { it.vendorGroup.isNotEmpty() }
            .groupBy { it.vendorGroup }
            .map { (vendor, list) -> vendor to list.size }
            .sortedByDescending { it.second }
    }

    val filteredOptions = remember(allOptions, searchQuery, selectedProviderFilter, selectedVendorFilter, selectedProfileId, selectedModelId) {
        val tokens = searchQuery.trim().lowercase().split(Regex("\\s+")).filter { it.isNotEmpty() }
        val matching = allOptions.filter { opt ->
            val provOk = selectedProviderFilter.isEmpty() || opt.profileId == selectedProviderFilter
            val vendOk = selectedVendorFilter.isEmpty() || opt.vendorGroup.equals(selectedVendorFilter, ignoreCase = true)
            val searchOk = tokens.isEmpty() ||
                tokens.all { tok ->
                    opt.modelId.lowercase().contains(tok) ||
                        opt.profileName.lowercase().contains(tok) ||
                        opt.vendorGroup.lowercase().contains(tok) ||
                        (opt.privacyBadge?.label?.lowercase()?.contains(tok) == true)
                }
            provOk && vendOk && searchOk
        }
        if (tokens.isEmpty()) {
            // Pin the currently active model at the top when unfiltered so it is immediately visible
            val activeList = matching.filter { it.profileId == selectedProfileId && it.modelId == selectedModelId }
            val restList = matching.filterNot { it.profileId == selectedProfileId && it.modelId == selectedModelId }
            activeList + restList
        } else {
            matching
        }
    }

    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(7.dp)
    ) {
        // 1. Search Bar + Clear Button
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Space.s6),
            verticalAlignment = Alignment.CenterVertically
        ) {
            SepiaTextField(
                value = searchQuery,
                onValueChange = { searchQuery = it },
                placeholder = placeholder,
                minHeight = 34.dp,
                modifier = Modifier.weight(1f),
                onEnterKeySend = { _, _ ->
                    val firstHit = filteredOptions.firstOrNull()
                    if (firstHit != null) {
                        onSelectModel(firstHit.profileId, firstHit.modelId)
                        true
                    } else false
                }
            )
            if (searchQuery.isNotEmpty() || selectedProviderFilter.isNotEmpty() || selectedVendorFilter.isNotEmpty()) {
                SepiaButton(
                    text = "×",
                    contentDescription = Strings.clearSearch,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = {
                        searchQuery = ""
                        selectedProviderFilter = ""
                        selectedVendorFilter = ""
                    }
                )
            }
        }

        // 2. Provider filters, when more than one provider has models; they wrap so none clip.
        if (providerCounts.size > 1) {
            @OptIn(ExperimentalLayoutApi::class)
            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
                verticalArrangement = Arrangement.spacedBy(5.dp)
            ) {
                SepiaChip(
                    label = Strings.allProviders2(allOptions.size),
                    selected = selectedProviderFilter.isEmpty(),
                    onClick = {
                        selectedProviderFilter = ""
                        selectedVendorFilter = ""
                    }
                )
                providerCounts.forEach { (profId, profName, count) ->
                    SepiaChip(
                        label = "$profName ($count)",
                        selected = selectedProviderFilter == profId,
                        onClick = {
                            selectedProviderFilter = if (selectedProviderFilter == profId) "" else profId
                            selectedVendorFilter = ""
                        }
                    )
                }
            }
        }

        // 3. Model family filters, when there are at least two families.
        if (vendorCounts.size >= 2) {
            @OptIn(ExperimentalLayoutApi::class)
            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
                verticalArrangement = Arrangement.spacedBy(5.dp)
            ) {
                SepiaChip(
                    label = Strings.allFamilies,
                    selected = selectedVendorFilter.isEmpty(),
                    onClick = { selectedVendorFilter = "" }
                )
                vendorCounts.forEach { (vendor, count) ->
                    SepiaChip(
                        label = "$vendor ($count)",
                        selected = selectedVendorFilter.equals(vendor, ignoreCase = true),
                        onClick = {
                            selectedVendorFilter = if (selectedVendorFilter.equals(vendor, ignoreCase = true)) "" else vendor
                        }
                    )
                }
            }
        }

        // 4. Bounded Vertical Scrollable Model List
        if (filteredOptions.isEmpty()) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(Radius.r8))
                    .background(p.elevatedBg.copy(alpha = 0.4f))
                    .padding(Space.s12)
            ) {
                BasicText(
                    text = if (allOptions.isEmpty()) {
                        Strings.noModelsConfiguredYetOpenManage
                    } else {
                        Strings.noModelsMatch(searchQuery)
                    },
                    style = p.uiLabelStyle
                )
            }
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = maxListHeight)
                    .clip(RoundedCornerShape(Radius.r8))
                    .background(p.sheetBg.copy(alpha = 0.45f))
                    .border(LineWidth.hairline, p.border.copy(alpha = 0.65f), RoundedCornerShape(Radius.r8))
                    .padding(Space.s6),
                verticalArrangement = Arrangement.spacedBy(Space.s4)
            ) {
                items(
                    items = filteredOptions,
                    key = { "${it.profileId}::${it.modelId}" },
                    contentType = { "model-option" }
                ) { opt ->
                    ModelPickerOptionRow(
                        opt = opt,
                        isSelected = opt.profileId == selectedProfileId && opt.modelId == selectedModelId,
                        showProviderBadge = providerCounts.size > 1,
                        onSelect = { onSelectModel(opt.profileId, opt.modelId) }
                    )
                }
            }
        }
    }
}

@Composable
private fun ModelPickerOptionRow(
    opt: SearchableModelOption,
    isSelected: Boolean,
    showProviderBadge: Boolean,
    onSelect: () -> Unit
) {
    val p = LocalParchmentPalette.current
    val rowInteraction = remember { MutableInteractionSource() }
    val isHovered by rowInteraction.collectIsHoveredAsState()
    val isPressed by rowInteraction.collectIsPressedAsState()
    val rowShape = RoundedCornerShape(Radius.r8)

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .kissClickable(
                pressScale = 0.99f,
                hoverScale = 1.005f,
                interactionSource = rowInteraction,
                selected = isSelected,
                onClick = onSelect
            )
            .clip(rowShape)
            .background(
                when {
                    isSelected -> p.accent.copy(alpha = 0.15f)
                    isPressed -> p.accent.copy(alpha = 0.12f)
                    isHovered -> p.accent.copy(alpha = 0.08f)
                    else -> p.cardBg.copy(alpha = 0.85f)
                }
            )
            .border(
                width = if (isSelected) 1.dp else LineWidth.hairline,
                color = when {
                    isSelected -> p.accent
                    isHovered -> p.accent.copy(alpha = 0.55f)
                    else -> p.border.copy(alpha = 0.5f)
                },
                shape = rowShape
            )
            .padding(horizontal = Space.s10, vertical = 7.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically
    ) {
        Row(
            modifier = Modifier.weight(1f).padding(end = Space.s8),
            horizontalArrangement = Arrangement.spacedBy(Space.s8),
            verticalAlignment = Alignment.CenterVertically
        ) {
            ModelLabSvgIcon(
                modelOrProvider = opt.modelId,
                tint = if (isSelected || isHovered) p.accent else p.narrationInk,
                size = IconSize.md,
                fallbackProtocolName = opt.profileName
            )
            BasicText(
                text = opt.modelId,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                style = p.uiBodyStyle.copy(
                    fontSize = 12.sp,
                    fontWeight = if (isSelected) FontWeight.Bold else FontWeight.Medium,
                    color = if (isSelected || isHovered) p.accent else p.primaryInk
                )
            )
        }

        Row(
            horizontalArrangement = Arrangement.spacedBy(5.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            if (showProviderBadge) {
                Box(
                    modifier = Modifier
                        .widthIn(max = 105.dp)
                        .clip(RoundedCornerShape(Radius.r6))
                        .background(p.sheetBg)
                        .padding(horizontal = Space.s6, vertical = Space.s2)
                ) {
                    BasicText(
                        text = opt.profileName,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = p.uiLabelStyle.copy(fontSize = 11.sp, color = p.narrationInk)
                    )
                }
            }
            if (opt.privacyBadge != null) {
                ModelPrivacyBadgeChip(badge = opt.privacyBadge)
            }
            Box(
                modifier = Modifier
                    .clip(RoundedCornerShape(Radius.r6))
                    .background(p.elevatedBg)
                    .padding(horizontal = 5.dp, vertical = Space.s2)
            ) {
                BasicText(
                    text = if (opt.contextWindowTokens <= 0) "∞" else formatCompactTokens(opt.contextWindowTokens),
                    style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFamily = p.codeFontFamily)
                )
            }
            if (isSelected) {
                KissVectorIcon(
                    icon = KissIcon.CHECK,
                    tint = p.accent,
                    size = IconSize.md
                )
            }
        }
    }
}
