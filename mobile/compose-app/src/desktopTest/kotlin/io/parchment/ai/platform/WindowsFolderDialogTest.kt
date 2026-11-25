package io.parchment.ai.platform

import java.lang.foreign.Arena
import kotlin.io.path.createTempDirectory
import kotlin.io.path.deleteIfExists
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class WindowsFolderDialogTest {
    /** Creates and configures native COM folder, multi-file open, and save dialogs without opening UI (`Show`). */
    @Test
    fun folderOpenAndSaveDialogsConfigureOnStaThread() {
        if (!WindowsNative.isWindows) return
        val existing = createTempDirectory("parchment-dialog-")
        try {
            WindowsFolderDialog.onStaThread {
                Arena.ofConfined().use { arena ->
                    WindowsFolderDialog.create(arena).use { dialog ->
                        val flags = WindowsFolderDialog.FOS_PICKFOLDERS or
                            WindowsFolderDialog.FOS_FORCEFILESYSTEM or
                            WindowsFolderDialog.FOS_PATHMUSTEXIST
                        dialog.setOptions(dialog.options() or flags)
                        assertEquals(flags, dialog.options() and flags)
                        dialog.setTitle("YL Axiom test")
                        assertTrue(dialog.setFolder(existing.toString()))
                        assertFalse(dialog.setFolder("Z:\\parchment-no-such-drive-or-folder"))
                    }

                    WindowsFolderDialog.create(arena).use { openDialog ->
                        val openFlags = WindowsFolderDialog.FOS_FORCEFILESYSTEM or
                            WindowsFolderDialog.FOS_PATHMUSTEXIST or
                            WindowsFolderDialog.FOS_FILEMUSTEXIST or
                            WindowsFolderDialog.FOS_ALLOWMULTISELECT
                        openDialog.setOptions(openDialog.options() or openFlags)
                        assertEquals(openFlags, openDialog.options() and openFlags)
                        val filters = WindowsFolderDialog.buildOpenFilters(listOf("png", "jpg"))
                        assertEquals(2, openDialog.setFileTypes(filters, selectedOneBasedIndex = 2))
                    }

                    WindowsFolderDialog.createSave(arena).use { saveDialog ->
                        val saveFlags = WindowsFolderDialog.FOS_FORCEFILESYSTEM or
                            WindowsFolderDialog.FOS_PATHMUSTEXIST or
                            WindowsFolderDialog.FOS_OVERWRITEPROMPT
                        saveDialog.setOptions(saveDialog.options() or saveFlags)
                        assertEquals(saveFlags, saveDialog.options() and saveFlags)
                        val idx = saveDialog.setFileTypes(
                            listOf(
                                FileDialogFilter("ZIP Archive (*.zip)", "*.zip"),
                                FileDialogFilter("All Files (*.*)", "*.*")
                            ),
                            selectedOneBasedIndex = 1
                        )
                        saveDialog.setDefaultExtension("zip")
                        saveDialog.setFileName("parchment-backup.zip")
                        assertEquals("parchment-backup.zip", saveDialog.fileName())
                        assertEquals(1, idx)
                    }
                }
            }
        } finally {
            existing.deleteIfExists()
        }
    }

    @Test
    fun buildOpenFiltersPreservesAllFilesFirstForGeneralAttachments() {
        val general = WindowsFolderDialog.buildOpenFilters(emptyList())
        assertEquals("*.*", general.first().pattern)
        assertTrue(general.size >= 3)

        val restricted = WindowsFolderDialog.buildOpenFilters(listOf(".PNG", "json", "png"))
        assertEquals("*.png;*.json", restricted.first().pattern)
        assertEquals("*.*", restricted.last().pattern)
    }
}
