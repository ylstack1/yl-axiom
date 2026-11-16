package io.parchment.ai.platform

import java.lang.foreign.Arena
import java.lang.foreign.FunctionDescriptor
import java.lang.foreign.Linker
import java.lang.foreign.MemorySegment
import java.lang.foreign.SymbolLookup
import java.lang.foreign.ValueLayout.ADDRESS
import java.lang.foreign.ValueLayout.JAVA_BYTE
import java.lang.foreign.ValueLayout.JAVA_INT
import java.lang.foreign.ValueLayout.JAVA_SHORT
import java.lang.invoke.MethodHandle

/**
 * The Windows folder picker (IFileOpenDialog with FOS_PICKFOLDERS), called through COM with FFM instead of the
 * Swing JFileChooser, which looks out of place and lacks Quick access, OneDrive and search. Everything happens on
 * a dedicated single-threaded-apartment thread, as COM dialogs require; any failure is reported so the caller can
 * fall back to JFileChooser.
 */
internal object WindowsFolderDialog {
    private const val CLSID_FILE_OPEN_DIALOG = "DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7"
    private const val IID_FILE_OPEN_DIALOG = "D57C7288-D4AD-4768-BE02-9D969532D960"
    private const val IID_SHELL_ITEM = "43826D1E-E718-42EE-BC55-A1E261C37BFE"
    private const val COINIT_APARTMENTTHREADED = 0x2
    private const val COINIT_DISABLE_OLE1DDE = 0x4
    private const val CLSCTX_INPROC_SERVER = 0x1
    const val FOS_PICKFOLDERS = 0x20
    const val FOS_FORCEFILESYSTEM = 0x40
    const val FOS_PATHMUSTEXIST = 0x800
    private const val SIGDN_FILESYSPATH = 0x80058000.toInt()
    /** HRESULT_FROM_WIN32(ERROR_CANCELLED): the person closed the dialog. */
    private const val HR_CANCELLED = 0x800704C7.toInt()

    // Vtable slots: IUnknown 0-2, IModalWindow.Show 3, IFileDialog 4-26 (SetOptions 9, GetOptions 10, SetFolder 12,
    // SetTitle 17, GetResult 20); IShellItem.GetDisplayName 5.
    private const val RELEASE = 2
    private const val SHOW = 3
    private const val SET_OPTIONS = 9
    private const val GET_OPTIONS = 10
    private const val SET_FOLDER = 12
    private const val SET_TITLE = 17
    private const val GET_RESULT = 20
    private const val GET_DISPLAY_NAME = 5

    private val linker: Linker by lazy { Linker.nativeLinker() }
    private fun function(library: String, name: String, descriptor: FunctionDescriptor): MethodHandle =
        linker.downcallHandle(SymbolLookup.libraryLookup(library, Arena.global()).find(name).orElseThrow(), descriptor)

    private val coInitializeEx by lazy { function("ole32", "CoInitializeEx", FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT)) }
    private val coUninitialize by lazy { function("ole32", "CoUninitialize", FunctionDescriptor.ofVoid()) }
    private val coCreateInstance by lazy {
        function("ole32", "CoCreateInstance", FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, JAVA_INT, ADDRESS, ADDRESS))
    }
    private val coTaskMemFree by lazy { function("ole32", "CoTaskMemFree", FunctionDescriptor.ofVoid(ADDRESS)) }
    private val shCreateItemFromParsingName by lazy {
        function("shell32", "SHCreateItemFromParsingName", FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, ADDRESS, ADDRESS))
    }

    /** Calls method [slot] of the COM object [self] (its first argument is always `this`). */
    private fun call(self: MemorySegment, slot: Int, descriptor: FunctionDescriptor, vararg args: Any): Any? {
        val vtable = self.reinterpret(ADDRESS.byteSize()).get(ADDRESS, 0)
        val method = vtable.reinterpret(ADDRESS.byteSize() * (slot + 1)).getAtIndex(ADDRESS, slot.toLong())
        return linker.downcallHandle(method, descriptor).invokeWithArguments(self, *args)
    }

    private fun check(hr: Int, what: String) {
        if (hr < 0) throw IllegalStateException("$what failed (HRESULT 0x${hr.toUInt().toString(16)})")
    }

    private fun guid(arena: Arena, text: String): MemorySegment {
        val hex = text.replace("-", "")
        val guid = arena.allocate(16, 4)
        guid.set(JAVA_INT, 0, hex.substring(0, 8).toLong(16).toInt())
        guid.set(JAVA_SHORT, 4, hex.substring(8, 12).toInt(16).toShort())
        guid.set(JAVA_SHORT, 6, hex.substring(12, 16).toInt(16).toShort())
        for (i in 0 until 8) guid.set(JAVA_BYTE, 8L + i, hex.substring(16 + 2 * i, 18 + 2 * i).toInt(16).toByte())
        return guid
    }

    private fun release(self: MemorySegment) {
        if (self.address() != 0L) call(self, RELEASE, FunctionDescriptor.of(JAVA_INT, ADDRESS))
    }

    /** One open IFileOpenDialog; [close] releases it. Must be used on the [onStaThread] thread that created it. */
    internal class Dialog(private val arena: Arena, val self: MemorySegment) : AutoCloseable {
        fun options(): Int {
            val out = arena.allocate(JAVA_INT)
            check(call(self, GET_OPTIONS, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), out) as Int, "GetOptions")
            return out.get(JAVA_INT, 0)
        }

        fun setOptions(options: Int) =
            check(call(self, SET_OPTIONS, FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT), options) as Int, "SetOptions")

        fun setTitle(title: String) = check(
            call(self, SET_TITLE, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), arena.allocateFrom(title, Charsets.UTF_16LE)) as Int,
            "SetTitle"
        )

        /** Opens at [directory]; a folder Windows cannot parse is skipped rather than failing the dialog. */
        fun setFolder(directory: String): Boolean {
            val item = arena.allocate(ADDRESS)
            val hr = shCreateItemFromParsingName.invokeWithArguments(
                arena.allocateFrom(directory, Charsets.UTF_16LE), MemorySegment.NULL, guid(arena, IID_SHELL_ITEM), item
            ) as Int
            if (hr < 0) return false
            val shellItem = item.get(ADDRESS, 0)
            try {
                return (call(self, SET_FOLDER, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), shellItem) as Int) >= 0
            } finally {
                release(shellItem)
            }
        }

        /** Shows the dialog (modal to [ownerHwnd] when non-zero); the chosen folder, or null when cancelled. */
        fun show(ownerHwnd: Long): String? {
            val hr = call(self, SHOW, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), MemorySegment.ofAddress(ownerHwnd)) as Int
            if (hr == HR_CANCELLED) return null
            check(hr, "Show")
            val item = arena.allocate(ADDRESS)
            check(call(self, GET_RESULT, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), item) as Int, "GetResult")
            val shellItem = item.get(ADDRESS, 0)
            try {
                val name = arena.allocate(ADDRESS)
                check(
                    call(shellItem, GET_DISPLAY_NAME, FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT, ADDRESS), SIGDN_FILESYSPATH, name) as Int,
                    "GetDisplayName"
                )
                val text = name.get(ADDRESS, 0)
                try {
                    return text.reinterpret(Long.MAX_VALUE).getString(0, Charsets.UTF_16LE)
                } finally {
                    coTaskMemFree.invokeWithArguments(text)
                }
            } finally {
                release(shellItem)
            }
        }

        override fun close() = release(self)
    }

    /** Creates the dialog in [arena]; the caller closes it. */
    internal fun create(arena: Arena): Dialog {
        val out = arena.allocate(ADDRESS)
        val hr = coCreateInstance.invokeWithArguments(
            guid(arena, CLSID_FILE_OPEN_DIALOG), MemorySegment.NULL, CLSCTX_INPROC_SERVER, guid(arena, IID_FILE_OPEN_DIALOG), out
        ) as Int
        check(hr, "CoCreateInstance(FileOpenDialog)")
        return Dialog(arena, out.get(ADDRESS, 0))
    }

    /** Runs [block] on a fresh thread joined to a single-threaded COM apartment, and waits for it. */
    internal fun <T> onStaThread(block: () -> T): T {
        var result: Result<T>? = null
        val thread = Thread({
            val hr = coInitializeEx.invokeWithArguments(MemorySegment.NULL, COINIT_APARTMENTTHREADED or COINIT_DISABLE_OLE1DDE) as Int
            try {
                result = runCatching(block)
            } finally {
                if (hr >= 0) coUninitialize.invokeWithArguments()
            }
        }, "parchment-folder-dialog")
        thread.start()
        thread.join()
        return checkNotNull(result) { "The folder dialog thread did not finish." }.getOrThrow()
    }

    /** The chosen folder, null when cancelled; a failure means the caller should use another picker. */
    fun pickFolder(title: String, initialDirectory: String, ownerHwnd: Long): Result<String?> = runCatching {
        check(WindowsNative.isWindows) { "Windows only." }
        onStaThread {
            Arena.ofConfined().use { arena ->
                create(arena).use { dialog ->
                    dialog.setOptions(dialog.options() or FOS_PICKFOLDERS or FOS_FORCEFILESYSTEM or FOS_PATHMUSTEXIST)
                    if (title.isNotBlank()) dialog.setTitle(title)
                    if (initialDirectory.isNotBlank()) dialog.setFolder(initialDirectory)
                    dialog.show(ownerHwnd)
                }
            }
        }
    }
}
