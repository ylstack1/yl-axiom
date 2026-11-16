package io.parchment.ai.platform

import io.ktor.utils.io.ByteReadChannel
import io.ktor.utils.io.jvm.javaio.toInputStream
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.decodeFromStream

@OptIn(ExperimentalSerializationApi::class)
internal suspend fun <T> decodeByteChannelJson(
    channel: ByteReadChannel,
    json: Json,
    deserializer: DeserializationStrategy<T>
): T = withContext(Dispatchers.IO) {
    channel.toInputStream().buffered(16 * 1024).use { stream ->
        json.decodeFromStream(deserializer, stream)
    }
}
