package io.parchment.ai.platform

import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.toComposeImageBitmap
import org.jetbrains.skia.Bitmap
import org.jetbrains.skia.Codec
import org.jetbrains.skia.Data
import org.jetbrains.skia.Image
import org.jetbrains.skia.Rect
import org.jetbrains.skia.SamplingMode
import org.jetbrains.skia.Surface
import java.io.File

/** Reads an image's dimensions once via Skia Codec and downsamples to target pixel bounds before GPU upload. */
internal object DesktopImageDecoder {
    private const val MAX_ENCODED_BYTES = 128L * 1024L * 1024L
    private const val MAX_SOURCE_PIXELS = 100_000_000L
    private const val MAX_DISPLAY_DIMENSION = 2560

    fun decode(file: File, maxDimensionPx: Int = MAX_DISPLAY_DIMENSION): ImageBitmap? {
        if (file.length() !in 1..MAX_ENCODED_BYTES) return null
        return decode(file.readBytes(), maxDimensionPx)
    }

    fun decode(bytes: ByteArray, maxDimensionPx: Int = MAX_DISPLAY_DIMENSION): ImageBitmap? {
        val cap = maxDimensionPx.coerceIn(64, MAX_DISPLAY_DIMENSION)
        return Data.makeFromBytes(bytes).use { data ->
            Codec.makeFromData(data).use { codec ->
                val width = codec.width
                val height = codec.height
                if (width <= 0 || height <= 0 || width.toLong() * height > MAX_SOURCE_PIXELS) return null
                Bitmap().use { sourceBitmap ->
                    sourceBitmap.allocPixels(codec.imageInfo)
                    codec.readPixels(sourceBitmap)
                    sourceBitmap.setImmutable()
                    val longest = maxOf(width, height)
                    if (longest <= cap) return sourceBitmap.asComposeImageBitmapOrCopy()
                    val targetWidth = (width.toLong() * cap / longest).toInt().coerceAtLeast(1)
                    val targetHeight = (height.toLong() * cap / longest).toInt().coerceAtLeast(1)
                    Image.makeFromBitmap(sourceBitmap).use { srcImage ->
                        Surface.makeRasterN32Premul(targetWidth, targetHeight).use { surface ->
                            surface.canvas.drawImageRect(
                                srcImage,
                                Rect.makeWH(width.toFloat(), height.toFloat()),
                                Rect.makeWH(targetWidth.toFloat(), targetHeight.toFloat()),
                                SamplingMode.MITCHELL,
                                null,
                                true
                            )
                            surface.makeImageSnapshot().use { it.toComposeImageBitmap() }
                        }
                    }
                }
            }
        }
    }

    private fun Bitmap.asComposeImageBitmapOrCopy(): ImageBitmap = Image.makeFromBitmap(this).use { it.toComposeImageBitmap() }
}
