package io.parchment.ai

import java.awt.Rectangle
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class WindowCaptionTest {
    @Test fun idleCaptionNamesTheChatAndARunShowsItsProgress() {
        assertEquals("Kyoto trip plan — YL Axiom", windowCaption("YL Axiom", " Kyoto trip plan "))
        assertEquals("Settings — YL Axiom", windowCaption("YL Axiom", "Settings"))
        assertEquals("YL Axiom", windowCaption("YL Axiom", ""))
        val running = "YL Axiom — model (+120 tok • 4s)"
        assertEquals(running, windowCaption(running, "Kyoto trip plan"))
    }

    @Test fun savedPositionOnAnUnpluggedMonitorReopensCentered() {
        val primary = Rectangle(0, 0, 1920, 1080)
        val leftMonitor = Rectangle(-1280, 0, 1280, 1024)
        assertTrue(savedPositionIsVisible(100, 100, 1200, listOf(primary)))
        assertTrue(savedPositionIsVisible(-1200, 50, 1000, listOf(primary, leftMonitor)), "negative x on a left monitor")
        assertFalse(savedPositionIsVisible(-1200, 50, 1000, listOf(primary)), "left monitor unplugged")
        assertFalse(savedPositionIsVisible(2400, 100, 1200, listOf(primary)), "right monitor unplugged")
        assertFalse(savedPositionIsVisible(1880, 100, 1200, listOf(primary)), "only a 40 dp sliver is on screen")
        assertFalse(savedPositionIsVisible(100, 100, 1200, emptyList()), "headless: center")
    }
}
