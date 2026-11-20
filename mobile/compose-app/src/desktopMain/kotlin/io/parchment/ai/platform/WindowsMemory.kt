package io.parchment.ai.platform

/** Total physical memory from Windows; the bundled runtime leaves out java.management, so this asks kernel32. */
internal object WindowsMemory {
    /** 0 when unknown (not Windows, or the call failed), which callers treat as "not low". */
    fun totalPhysicalBytes(): Long = runCatching {
        if (!WindowsNative.isWindows) 0L else WindowsNative.totalPhysicalBytes()
    }.getOrDefault(0L)
}
