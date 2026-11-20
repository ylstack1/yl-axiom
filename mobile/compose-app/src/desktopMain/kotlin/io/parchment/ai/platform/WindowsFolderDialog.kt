package io.parchment.ai.platform

import java.lang.foreign.Arena
import java.lang.foreign.FunctionDescriptor
import java.lang.foreign.Linker
import java.lang.foreign.MemoryLayout
import java.lang.foreign.MemorySegment
import java.lang.foreign.StructLayout
import java.lang.foreign.SymbolLookup
import java.lang.foreign.ValueLayout.ADDRESS
import java.lang.foreign.ValueLayout.JAVA_BYTE
import java.lang.foreign.ValueLayout.JAVA_INT
import java.lang.foreign.ValueLayout.JAVA_SHORT
import java.lang.invoke.MethodHandle

/** A single entry in the native Win32 file-type filter dropdown (`COMDLG_FILTERSPEC`). */
internal data class FileDialogFilter(val label: String, val pattern: String)

/**
 * Native Windows 10/11 common file and folder dialogs (`IFileOpenDialog` and `IFileSaveDialog`), called directly
 * through COM with JDK 25's Foreign Function & Memory API instead of AWT `FileDialog` or Swing `JFileChooser`.
 * Gives Quick access, OneDrive, Recent files, thumbnails, native dark/light DWM chrome, proper filter dropdowns
 * without polluting the file-name edit box, and multi-file selection (`IShellItemArray`).
 *
 * Every COM call runs on a dedicated single-threaded-apartment (STA) thread and closes its native handles in
 * `finally`; any failure returns `Result.failure` so callers can fall back to AWT/Swing on non-Windows hosts.
 */
internal object WindowsFolderDialog {
    private const val CLSID_FILE_OPEN_DIALOG = "DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7"
    private const val IID_FILE_OPEN_DIALOG = "D57C7288-D4AD-4768-BE02-9D969532D960"
    private const val CLSID_FILE_SAVE_DIALOG = "C0B4E2F3-BA21-4773-8DBA-335EC946EB8B"
    private const val IID_FILE_SAVE_DIALOG = "84BCCD23-5FDE-4CDB-AEA4-AF64B83D78AB"
    private const val IID_SHELL_ITEM = "43826D1E-E718-42EE-BC55-A1E261C37BFE"
    private const val COINIT_APARTMENTTHREADED = 0x2
    private const val COINIT_DISABLE_OLE1DDE = 0x4
    private const val CLSCTX_INPROC_SERVER = 0x1

    const val FOS_OVERWRITEPROMPT = 0x2
    const val FOS_PICKFOLDERS = 0x20
    const val FOS_FORCEFILESYSTEM = 0x40
    const val FOS_ALLOWMULTISELECT = 0x200
    const val FOS_PATHMUSTEXIST = 0x800
    const val FOS_FILEMUSTEXIST = 0x1000

    private const val SIGDN_FILESYSPATH = 0x80058000.toInt()

    /** HRESULT_FROM_WIN32(ERROR_CANCELLED): the person closed the dialog. */
    private const val HR_CANCELLED = 0x800704C7.toInt()

    // Vtable slots:
    // IUnknown 0-2, IModalWindow.Show 3,
    // IFileDialog 4-26 (SetFileTypes 4, SetFileTypeIndex 5, GetFileTypeIndex 6, SetOptions 9, GetOptions 10,
    // SetFolder 12, SetFileName 15, GetFileName 16, SetTitle 17, GetResult 20, SetDefaultExtension 22),
    // IFileOpenDialog.GetResults 27; IShellItem.GetDisplayName 5; IShellItemArray (GetCount 7, GetItemAt 8).
    private const val RELEASE = 2
    private const val SHOW = 3
    private const val SET_FILE_TYPES = 4
    private const val SET_FILE_TYPE_INDEX = 5
    private const val GET_FILE_TYPE_INDEX = 6
    private const val SET_OPTIONS = 9
    private const val GET_OPTIONS = 10
    private const val SET_FOLDER = 12
    private const val SET_FILE_NAME = 15
    private const val GET_FILE_NAME = 16
    private const val SET_TITLE = 17
    private const val GET_RESULT = 20
    private const val SET_DEFAULT_EXTENSION = 22
    private const val GET_RESULTS = 27

    private const val GET_DISPLAY_NAME = 5
    private const val ARRAY_GET_COUNT = 7
    private const val ARRAY_GET_ITEM_AT = 8

    private val FILTER_SPEC_LAYOUT: StructLayout = MemoryLayout.structLayout(
        ADDRESS.withName("pszName"),
        ADDRESS.withName("pszSpec")
    )

    private val linker: Linker by lazy { Linker.nativeLinker() }
    private fun function(library: String, name: String, descriptor: FunctionDescriptor): MethodHandle = linker.downcallHandle(SymbolLookup.libraryLookup(library, Arena.global()).find(name).orElseThrow(), descriptor)

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

    private fun readCoTaskString(ptrHolder: MemorySegment): String {
        val text = ptrHolder.get(ADDRESS, 0)
        try {
            return text.reinterpret(Long.MAX_VALUE).getString(0, Charsets.UTF_16LE)
        } finally {
            coTaskMemFree.invokeWithArguments(text)
        }
    }

    private fun readShellItemPath(arena: Arena, shellItem: MemorySegment): String {
        val name = arena.allocate(ADDRESS)
        check(
            call(shellItem, GET_DISPLAY_NAME, FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT, ADDRESS), SIGDN_FILESYSPATH, name) as Int,
            "GetDisplayName"
        )
        return readCoTaskString(name)
    }

    /** One open `IFileDialog` (`IFileOpenDialog` or `IFileSaveDialog`); [close] releases it on the STA thread. */
    internal class Dialog(private val arena: Arena, val self: MemorySegment) : AutoCloseable {
        fun options(): Int {
            val out = arena.allocate(JAVA_INT)
            check(call(self, GET_OPTIONS, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), out) as Int, "GetOptions")
            return out.get(JAVA_INT, 0)
        }

        fun setOptions(options: Int) = check(call(self, SET_OPTIONS, FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT), options) as Int, "SetOptions")

        fun setTitle(title: String) = check(
            call(self, SET_TITLE, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), arena.allocateFrom(title, Charsets.UTF_16LE)) as Int,
            "SetTitle"
        )

        fun setFileName(fileName: String) = check(
            call(self, SET_FILE_NAME, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), arena.allocateFrom(fileName, Charsets.UTF_16LE)) as Int,
            "SetFileName"
        )

        fun fileName(): String {
            val out = arena.allocate(ADDRESS)
            check(call(self, GET_FILE_NAME, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), out) as Int, "GetFileName")
            return readCoTaskString(out)
        }

        fun setDefaultExtension(extension: String) {
            val clean = extension.trim().trimStart('.')
            if (clean.isEmpty()) return
            check(
                call(self, SET_DEFAULT_EXTENSION, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), arena.allocateFrom(clean, Charsets.UTF_16LE)) as Int,
                "SetDefaultExtension"
            )
        }

        fun setFileTypes(filters: List<FileDialogFilter>, selectedOneBasedIndex: Int = 1): Int {
            if (filters.isEmpty()) return 0
            val specs = arena.allocate(FILTER_SPEC_LAYOUT, filters.size.toLong())
            val stride = FILTER_SPEC_LAYOUT.byteSize()
            filters.forEachIndexed { idx, filter ->
                val base = idx * stride
                specs.set(ADDRESS, base, arena.allocateFrom(filter.label, Charsets.UTF_16LE))
                specs.set(ADDRESS, base + ADDRESS.byteSize(), arena.allocateFrom(filter.pattern, Charsets.UTF_16LE))
            }
            check(
                call(self, SET_FILE_TYPES, FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT, ADDRESS), filters.size, specs) as Int,
                "SetFileTypes"
            )
            check(
                call(self, SET_FILE_TYPE_INDEX, FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT), selectedOneBasedIndex.coerceIn(1, filters.size)) as Int,
                "SetFileTypeIndex"
            )
            val out = arena.allocate(JAVA_INT)
            check(call(self, GET_FILE_TYPE_INDEX, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), out) as Int, "GetFileTypeIndex")
            return out.get(JAVA_INT, 0)
        }

        /** Opens at [directory]; a folder Windows cannot parse is skipped rather than failing the dialog. */
        fun setFolder(directory: String): Boolean {
            val item = arena.allocate(ADDRESS)
            val hr = shCreateItemFromParsingName.invokeWithArguments(
                arena.allocateFrom(directory, Charsets.UTF_16LE),
                MemorySegment.NULL,
                guid(arena, IID_SHELL_ITEM),
                item
            ) as Int
            if (hr < 0) return false
            val shellItem = item.get(ADDRESS, 0)
            try {
                return (call(self, SET_FOLDER, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), shellItem) as Int) >= 0
            } finally {
                release(shellItem)
            }
        }

        /** Shows the dialog (modal to [ownerHwnd] when non-zero); the single chosen path, or null when cancelled. */
        fun show(ownerHwnd: Long): String? {
            val hr = call(self, SHOW, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), MemorySegment.ofAddress(ownerHwnd)) as Int
            if (hr == HR_CANCELLED) return null
            check(hr, "Show")
            val item = arena.allocate(ADDRESS)
            check(call(self, GET_RESULT, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), item) as Int, "GetResult")
            val shellItem = item.get(ADDRESS, 0)
            try {
                return readShellItemPath(arena, shellItem)
            } finally {
                release(shellItem)
            }
        }

        /** Shows an `IFileOpenDialog` and returns every chosen filesystem path (empty list when cancelled). */
        fun showMultiple(ownerHwnd: Long): List<String> {
            val hr = call(self, SHOW, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), MemorySegment.ofAddress(ownerHwnd)) as Int
            if (hr == HR_CANCELLED) return emptyList()
            check(hr, "Show")
            val arrayOut = arena.allocate(ADDRESS)
            check(call(self, GET_RESULTS, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), arrayOut) as Int, "GetResults")
            val itemArray = arrayOut.get(ADDRESS, 0)
            try {
                val countOut = arena.allocate(JAVA_INT)
                check(call(itemArray, ARRAY_GET_COUNT, FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS), countOut) as Int, "IShellItemArray.GetCount")
                val count = countOut.get(JAVA_INT, 0).coerceAtLeast(0)
                val paths = ArrayList<String>(count)
                val itemOut = arena.allocate(ADDRESS)
                for (index in 0 until count) {
                    check(
                        call(itemArray, ARRAY_GET_ITEM_AT, FunctionDescriptor.of(JAVA_INT, ADDRESS, JAVA_INT, ADDRESS), index, itemOut) as Int,
                        "IShellItemArray.GetItemAt($index)"
                    )
                    val shellItem = itemOut.get(ADDRESS, 0)
                    try {
                        paths.add(readShellItemPath(arena, shellItem))
                    } finally {
                        release(shellItem)
                    }
                }
                return paths
            } finally {
                release(itemArray)
            }
        }

        override fun close() = release(self)
    }

    /** Creates an `IFileOpenDialog` in [arena]; the caller closes it. */
    internal fun create(arena: Arena): Dialog {
        val out = arena.allocate(ADDRESS)
        val hr = coCreateInstance.invokeWithArguments(
            guid(arena, CLSID_FILE_OPEN_DIALOG),
            MemorySegment.NULL,
            CLSCTX_INPROC_SERVER,
            guid(arena, IID_FILE_OPEN_DIALOG),
            out
        ) as Int
        check(hr, "CoCreateInstance(FileOpenDialog)")
        return Dialog(arena, out.get(ADDRESS, 0))
    }

    /** Creates an `IFileSaveDialog` in [arena]; the caller closes it. */
    internal fun createSave(arena: Arena): Dialog {
        val out = arena.allocate(ADDRESS)
        val hr = coCreateInstance.invokeWithArguments(
            guid(arena, CLSID_FILE_SAVE_DIALOG),
            MemorySegment.NULL,
            CLSCTX_INPROC_SERVER,
            guid(arena, IID_FILE_SAVE_DIALOG),
            out
        ) as Int
        check(hr, "CoCreateInstance(FileSaveDialog)")
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
        }, "parchment-file-dialog")
        thread.start()
        thread.join()
        return checkNotNull(result) { "The file dialog thread did not finish." }.getOrThrow()
    }

    /** Builds clean native filter specs for [allowedExtensions] (empty or `*` offers rich category presets). */
    internal fun buildOpenFilters(allowedExtensions: List<String>): List<FileDialogFilter> {
        val clean = allowedExtensions.map { it.trim().trimStart('.').lowercase() }.filter { it.isNotEmpty() && it != "*" }.distinct()
        if (clean.isEmpty()) {
            return listOf(
                FileDialogFilter("All Files (*.*)", "*.*"),
                FileDialogFilter(
                    "Images, Audio & Video",
                    "*.png;*.jpg;*.jpeg;*.webp;*.gif;*.bmp;*.mp4;*.webm;*.mov;*.mkv;*.mp3;*.wav;*.ogg;*.flac"
                ),
                FileDialogFilter(
                    "Documents, Data & Code",
                    "*.pdf;*.md;*.txt;*.json;*.csv;*.xml;*.yaml;*.yml;*.kt;*.kts;*.java;*.py;*.ts;*.tsx;*.js;*.rs;*.go;*.c;*.cpp;*.h;*.cs;*.html;*.css;*.sql"
                )
            )
        }
        val spec = clean.joinToString(";") { "*.$it" }
        val display = clean.joinToString(", ") { ".$it" }
        return listOf(
            FileDialogFilter("Supported Files ($display)", spec),
            FileDialogFilter("All Files (*.*)", "*.*")
        )
    }

    /** The chosen folder, null when cancelled; a failure means the caller should use another picker. */
    fun pickFolder(title: String, initialDirectory: String, ownerHwnd: Long): Result<String?> = runCatching {
        check(WindowsNative.isWindows && !java.awt.GraphicsEnvironment.isHeadless()) { "Interactive Windows desktop required." }
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

    /** Native Windows file open dialog (`IFileOpenDialog`); empty list when cancelled. */
    fun pickFiles(
        title: String,
        filters: List<FileDialogFilter>,
        allowMultiple: Boolean,
        ownerHwnd: Long,
        initialDirectory: String = ""
    ): Result<List<String>> = runCatching {
        check(WindowsNative.isWindows && !java.awt.GraphicsEnvironment.isHeadless()) { "Interactive Windows desktop required." }
        onStaThread {
            Arena.ofConfined().use { arena ->
                create(arena).use { dialog ->
                    var flags = dialog.options() or FOS_FORCEFILESYSTEM or FOS_PATHMUSTEXIST or FOS_FILEMUSTEXIST
                    if (allowMultiple) flags = flags or FOS_ALLOWMULTISELECT
                    dialog.setOptions(flags)
                    if (title.isNotBlank()) dialog.setTitle(title)
                    if (filters.isNotEmpty()) dialog.setFileTypes(filters)
                    if (initialDirectory.isNotBlank()) dialog.setFolder(initialDirectory)
                    if (allowMultiple) dialog.showMultiple(ownerHwnd) else listOfNotNull(dialog.show(ownerHwnd))
                }
            }
        }
    }

    /** Native Windows file save dialog (`IFileSaveDialog`); null when cancelled. */
    fun saveFile(
        title: String,
        defaultFileName: String,
        ownerHwnd: Long,
        initialDirectory: String = ""
    ): Result<String?> = runCatching {
        check(WindowsNative.isWindows && !java.awt.GraphicsEnvironment.isHeadless()) { "Interactive Windows desktop required." }
        onStaThread {
            Arena.ofConfined().use { arena ->
                createSave(arena).use { dialog ->
                    dialog.setOptions(dialog.options() or FOS_FORCEFILESYSTEM or FOS_PATHMUSTEXIST or FOS_OVERWRITEPROMPT)
                    if (title.isNotBlank()) dialog.setTitle(title)
                    val ext = defaultFileName.substringAfterLast('.', "").trim().lowercase()
                    if (ext.isNotEmpty()) {
                        dialog.setFileTypes(
                            listOf(
                                FileDialogFilter("${ext.uppercase()} File (*.$ext)", "*.$ext"),
                                FileDialogFilter("All Files (*.*)", "*.*")
                            )
                        )
                        dialog.setDefaultExtension(ext)
                    }
                    if (defaultFileName.isNotBlank()) dialog.setFileName(defaultFileName)
                    if (initialDirectory.isNotBlank()) dialog.setFolder(initialDirectory)
                    dialog.show(ownerHwnd)
                }
            }
        }
    }
}
