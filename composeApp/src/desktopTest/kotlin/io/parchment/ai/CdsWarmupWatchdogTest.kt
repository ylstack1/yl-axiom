package io.parchment.ai

import java.io.File
import java.nio.file.Files
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The warm-up's watchdog must not leave a partial class archive: the JVM keeps any valid archive, so a partial one
 * would slow every later start. `Runtime.halt` still dumps it; `TerminateProcess` (WindowsNative) does not.
 */
class CdsWarmupWatchdogTest {
    private val onWindows = System.getProperty("os.name").orEmpty().startsWith("Windows", ignoreCase = true)

    /** Same downcalls as WindowsNative.terminateCurrentProcess, in a single-file program the JDK runs from source. */
    private val probe = """
        import java.lang.foreign.*;
        import java.lang.invoke.MethodHandle;
        public class Probe {
            public static void main(String[] args) throws Throwable {
                if (args[0].equals("halt")) Runtime.getRuntime().halt(3);
                Linker linker = Linker.nativeLinker();
                SymbolLookup kernel32 = SymbolLookup.libraryLookup("kernel32", Arena.global());
                MethodHandle current = linker.downcallHandle(kernel32.find("GetCurrentProcess").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.ADDRESS));
                MethodHandle terminate = linker.downcallHandle(kernel32.find("TerminateProcess").orElseThrow(),
                    FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.JAVA_INT));
                MemorySegment self = (MemorySegment) current.invokeExact();
                int ignored = (int) terminate.invokeExact(self, 3);
            }
        }
    """.trimIndent()

    private fun run(dir: File, mode: String): Pair<Int, Boolean> {
        val archive = File(dir, "$mode.jsa")
        val java = File(System.getProperty("java.home"), "bin/java.exe").takeIf { it.isFile }
            ?: File(System.getProperty("java.home"), "bin/java")
        val process = ProcessBuilder(
            java.path,
            "-XX:ArchiveClassesAtExit=${archive.path}",
            "--enable-native-access=ALL-UNNAMED",
            File(dir, "Probe.java").path,
            mode
        ).redirectErrorStream(true).start()
        process.inputStream.bufferedReader().readText()
        assertTrue(process.waitFor(60, TimeUnit.SECONDS), "probe did not finish")
        return process.exitValue() to archive.isFile
    }

    @Test fun terminatingWritesNoArchiveWhileHaltDoes() {
        if (!onWindows) return
        val dir = Files.createTempDirectory("parchment-watchdog-test-").toFile()
        try {
            File(dir, "Probe.java").writeText(probe)
            val (haltExit, haltArchive) = run(dir, "halt")
            assertEquals(3, haltExit)
            assertTrue(haltArchive, "premise: Runtime.halt still dumps the dynamic archive")
            val (terminateExit, terminateArchive) = run(dir, "terminate")
            assertEquals(3, terminateExit)
            assertFalse(terminateArchive, "TerminateProcess must end the process before any archive is written")
        } finally {
            dir.deleteRecursively()
        }
    }
}
