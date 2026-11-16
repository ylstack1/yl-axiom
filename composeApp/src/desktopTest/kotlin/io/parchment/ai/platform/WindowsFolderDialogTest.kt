package io.parchment.ai.platform

import java.lang.foreign.Arena
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertTrue

class WindowsFolderDialogTest {
    /** Every COM call except Show (which waits for a person): create, options round trip, title, start folder, release. */
    @Test fun theNativeFolderDialogIsCreatedAndConfiguredThroughCom() {
        if (!WindowsNative.isWindows) return
        val start = Files.createTempDirectory("parchment-folder-dialog-").toFile()
        try {
            WindowsFolderDialog.onStaThread {
                Arena.ofConfined().use { arena ->
                    WindowsFolderDialog.create(arena).use { dialog ->
                        val wanted = WindowsFolderDialog.FOS_PICKFOLDERS or WindowsFolderDialog.FOS_FORCEFILESYSTEM
                        dialog.setOptions(dialog.options() or wanted)
                        assertTrue(dialog.options() and wanted == wanted, "folder-picking options must stick")
                        dialog.setTitle("Choose a workspace")
                        assertTrue(dialog.setFolder(start.absolutePath), "the dialog must accept a start folder")
                    }
                }
            }
        } finally {
            start.delete()
        }
    }
}
