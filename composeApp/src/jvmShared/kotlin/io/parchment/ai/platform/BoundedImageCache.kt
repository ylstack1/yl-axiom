package io.parchment.ai.platform

import androidx.compose.ui.graphics.ImageBitmap
import java.io.File

/** Thread-safe bounded image cache keyed by logical path or resolved absolute file path. */
/** Memory-bounded LRU decoded image cache shared by Android and desktop storage. */
internal class BoundedImageCache(
    private val limitsProvider: () -> Pair<Int, Long>
) {
    private val entries = LinkedHashMap<String, ImageBitmap>(16, 0.75f, true)
    private val aliasToCanonical = HashMap<String, String>()
    private var totalBytes: Long = 0L

    fun get(key: String): ImageBitmap? {
        val normalized = normalizeCacheKey(key)
        if (normalized.isEmpty()) return null
        return synchronized(entries) {
            val canonical = aliasToCanonical[normalized] ?: normalized
            entries[canonical]
        }
    }

    fun remove(key: String) {
        val normalized = normalizeCacheKey(key)
        if (normalized.isEmpty()) return
        synchronized(entries) {
            val canonical = aliasToCanonical.remove(normalized) ?: normalized
            val removed = entries.remove(canonical)
            if (removed != null) {
                totalBytes = (totalBytes - bitmapBytes(removed)).coerceAtLeast(0L)
            }
            aliasToCanonical.values.removeAll { it == canonical }
        }
    }

    fun clear() = synchronized(entries) {
        entries.clear()
        aliasToCanonical.clear()
        totalBytes = 0L
    }

    fun getOrLoad(
        filePath: String,
        resolveFile: (String) -> File?,
        decode: (File) -> ImageBitmap?
    ): ImageBitmap? {
        val normalizedInput = normalizeCacheKey(filePath)
        if (normalizedInput.isEmpty()) return null
        get(normalizedInput)?.let { return it }
        return runCatching {
            val file = resolveFile(filePath) ?: return null
            val canonical = normalizeCacheKey(file.absolutePath).ifEmpty { normalizedInput }
            synchronized(entries) {
                entries[canonical]?.let { hit ->
                    if (normalizedInput != canonical) {
                        aliasToCanonical[normalizedInput] = canonical
                    }
                    return hit
                }
            }
            val bitmap = decode(file) ?: return null
            put(canonical, normalizedInput, bitmap)
            bitmap
        }.getOrNull()
    }

    private fun put(canonical: String, alias: String, bitmap: ImageBitmap) = synchronized(entries) {
        val previous = entries.put(canonical, bitmap)
        if (previous != null) {
            totalBytes = (totalBytes - bitmapBytes(previous)).coerceAtLeast(0L)
        }
        totalBytes += bitmapBytes(bitmap)
        if (alias != canonical) {
            aliasToCanonical[alias] = canonical
        }
        val (maxImages, maxBytes) = limitsProvider()
        val iterator = entries.entries.iterator()
        while ((entries.size > maxImages || totalBytes > maxBytes) && iterator.hasNext()) {
            val eldest = iterator.next()
            if (eldest.key == canonical) break
            iterator.remove()
            totalBytes = (totalBytes - bitmapBytes(eldest.value)).coerceAtLeast(0L)
            aliasToCanonical.values.removeAll { it == eldest.key }
        }
    }

    private fun bitmapBytes(bitmap: ImageBitmap): Long =
        bitmap.width.toLong() * bitmap.height * 4L

    private fun normalizeCacheKey(raw: String): String {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) return ""
        return runCatching { File(trimmed).toPath().normalize().toString() }.getOrElse { trimmed }
    }
}
