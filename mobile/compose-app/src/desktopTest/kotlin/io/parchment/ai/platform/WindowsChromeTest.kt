package io.parchment.ai.platform

import java.awt.Frame
import java.awt.GraphicsEnvironment
import java.lang.foreign.Arena
import java.lang.foreign.FunctionDescriptor
import java.lang.foreign.Linker
import java.lang.foreign.MemorySegment
import java.lang.foreign.SymbolLookup
import java.lang.foreign.ValueLayout.ADDRESS
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class WindowsChromeTest {
    private val onWindows = System.getProperty("os.name").orEmpty().startsWith("Windows", ignoreCase = true)

    /** The frame's HWND, found by its unique title through user32!FindWindowW. */
    private fun windowHandle(title: String): Long = Arena.ofConfined().use { arena ->
        val findWindow = Linker.nativeLinker().downcallHandle(
            SymbolLookup.libraryLookup("user32", arena).find("FindWindowW").orElseThrow(),
            FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS)
        )
        val wideTitle = arena.allocateFrom(title, Charsets.UTF_16LE) // allocateFrom adds the terminator
        (findWindow.invokeWithArguments(MemorySegment.NULL, wideTitle) as MemorySegment).address()
    }

    @Test fun nativeCaptionCallsAreAcceptedByWindows() {
        if (!onWindows || GraphicsEnvironment.isHeadless()) return
        val title = "parchment-caption-test-" + System.nanoTime()
        val frame = Frame(title)
        try {
            frame.addNotify()
            val hwnd = windowHandle(title)
            assertTrue(hwnd != 0L, "test window not found")
            val results = WindowsChrome.applyCaption(hwnd, 0xF4ECD8, 0x261C14, 0xD5C4A1, dark = false)
            assertEquals(0, results["text"], "caption text must take the theme's ink: $results")
            assertEquals(0, results["darkMode"], "caption buttons must follow the theme: $results")
        } finally { frame.dispose() }
    }

    @Test fun memoryStatusLayoutMatchesWindows() {
        assertEquals(64L, WindowsNative.MEMORY_STATUS_EX.byteSize(), "MEMORYSTATUSEX is 64 bytes on x64")
        if (!onWindows) return
        val total = WindowsMemory.totalPhysicalBytes()
        assertTrue(total > 512L * 1024 * 1024, "installed memory should be reported, got $total")
    }
}
