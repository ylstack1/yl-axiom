package io.parchment.ai.platform

import java.lang.foreign.Arena
import java.lang.foreign.FunctionDescriptor
import java.lang.foreign.Linker
import java.lang.foreign.MemoryLayout
import java.lang.foreign.MemorySegment
import java.lang.foreign.StructLayout
import java.lang.foreign.SymbolLookup
import java.lang.foreign.ValueLayout.ADDRESS
import java.lang.foreign.ValueLayout.JAVA_INT
import java.lang.foreign.ValueLayout.JAVA_LONG
import java.lang.invoke.MethodHandle

/**
 * The three Win32 calls the desktop app needs, made directly through the JDK's Foreign Function & Memory API
 * (final since JDK 22; the packaged runtime is JDK 25). It replaces JNA and its bundled native library, and the
 * PowerShell child process that used to hold the keep-awake request. Handles are created on first use and only on
 * Windows; every caller treats a failure as "unavailable". Needs `--enable-native-access=ALL-UNNAMED`, which the
 * packaged app already passes (without it JDK 25 only warns).
 */
internal object WindowsNative {
    val isWindows: Boolean = System.getProperty("os.name").orEmpty().startsWith("Windows", ignoreCase = true)

    private val linker: Linker by lazy { Linker.nativeLinker() }

    private fun downcall(library: String, name: String, descriptor: FunctionDescriptor): MethodHandle {
        check(isWindows) { "Windows only." }
        val symbol = SymbolLookup.libraryLookup(library, Arena.global()).find(name).orElseThrow()
        return linker.downcallHandle(symbol, descriptor)
    }

    /** HRESULT DwmSetWindowAttribute(HWND, DWORD attribute, LPCVOID value, DWORD size). */
    private val dwmSetWindowAttribute by lazy {
        downcall("dwmapi", "DwmSetWindowAttribute", FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT, ADDRESS, JAVA_INT))
    }

    /** BOOL GlobalMemoryStatusEx(LPMEMORYSTATUSEX). */
    private val globalMemoryStatusEx by lazy {
        downcall("kernel32", "GlobalMemoryStatusEx", FunctionDescriptor.of(JAVA_INT, ADDRESS))
    }

    /** EXECUTION_STATE SetThreadExecutionState(EXECUTION_STATE): the previous state, or 0 on failure. */
    private val setThreadExecutionStateHandle by lazy {
        downcall("kernel32", "SetThreadExecutionState", FunctionDescriptor.of(JAVA_INT, JAVA_INT))
    }

    /** HANDLE GetCurrentProcess(): a pseudo-handle, never closed. */
    private val getCurrentProcess by lazy {
        downcall("kernel32", "GetCurrentProcess", FunctionDescriptor.of(ADDRESS))
    }

    /** BOOL TerminateProcess(HANDLE, UINT exitCode). */
    private val terminateProcess by lazy {
        downcall("kernel32", "TerminateProcess", FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT))
    }

    /**
     * Ends this process at once with [exitCode]. Unlike `Runtime.halt` (which still runs the JVM's `before_exit`,
     * where the dynamic class archive is dumped), nothing more runs: no archive, no shutdown hooks. Returns only if
     * Windows refused, in which case the caller decides what to do next.
     */
    fun terminateCurrentProcess(exitCode: Int): Boolean {
        val self = getCurrentProcess.invokeWithArguments() as MemorySegment
        return (terminateProcess.invokeWithArguments(self, exitCode) as Int) != 0
    }

    /** MEMORYSTATUSEX: two DWORDs then seven DWORDLONGs, 64 bytes with natural alignment. */
    val MEMORY_STATUS_EX: StructLayout = MemoryLayout.structLayout(
        JAVA_INT.withName("dwLength"),
        JAVA_INT.withName("dwMemoryLoad"),
        JAVA_LONG.withName("ullTotalPhys"),
        JAVA_LONG.withName("ullAvailPhys"),
        JAVA_LONG.withName("ullTotalPageFile"),
        JAVA_LONG.withName("ullAvailPageFile"),
        JAVA_LONG.withName("ullTotalVirtual"),
        JAVA_LONG.withName("ullAvailVirtual"),
        JAVA_LONG.withName("ullAvailExtendedVirtual")
    )
    private val TOTAL_PHYS_OFFSET = MEMORY_STATUS_EX.byteOffset(MemoryLayout.PathElement.groupElement("ullTotalPhys"))

    /** Sets one 4-byte DWM window attribute; returns the HRESULT (0 = success). */
    fun setWindowAttribute(hwnd: Long, attribute: Int, value: Int): Int = Arena.ofConfined().use { arena ->
        val buffer = arena.allocate(JAVA_INT)
        buffer.set(JAVA_INT, 0, value)
        dwmSetWindowAttribute.invokeWithArguments(MemorySegment.ofAddress(hwnd), attribute, buffer, Int.SIZE_BYTES) as Int
    }

    /** Installed physical memory in bytes, or 0 when the call fails. */
    fun totalPhysicalBytes(): Long = Arena.ofConfined().use { arena ->
        val status = arena.allocate(MEMORY_STATUS_EX)
        status.set(JAVA_INT, 0, MEMORY_STATUS_EX.byteSize().toInt())
        val ok = globalMemoryStatusEx.invokeWithArguments(status) as Int
        if (ok != 0) status.get(JAVA_LONG, TOTAL_PHYS_OFFSET) else 0L
    }

    /** Applies to the calling thread only, and Windows drops it when that thread ends. */
    fun setThreadExecutionState(flags: Int): Int = setThreadExecutionStateHandle.invokeWithArguments(flags) as Int

    const val ES_SYSTEM_REQUIRED = 0x00000001
    const val ES_CONTINUOUS = 0x80000000.toInt()
}
