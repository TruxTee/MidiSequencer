package dev.sequencer.pcr32

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EncoderDragTest {
    @Test fun slowDragIsOneStepPerFourDp() {
        val d = EncoderDrag()
        // 40 dp upward in 2 dp moves, 16 ms apart = 0.125 dp/ms: below the acceleration threshold
        val steps = (1..20).sumOf { d.move(2f, 16) }
        assertEquals(10, steps)
    }

    @Test fun downwardDragIsNegative() {
        val d = EncoderDrag()
        assertEquals(-5, (1..10).sumOf { d.move(-2f, 16) })
    }

    @Test fun fastFlickCoversTheWholeRange() {
        val d = EncoderDrag()
        // 120 dp in 60 ms (2 dp/ms): gain capped at 8 -> 120 / 4 * 8 = 240 steps
        val steps = (1..6).sumOf { d.move(20f, 10) }
        assertEquals(240, steps)
        assertTrue(steps >= 127)
    }

    @Test fun moderateSpeedAccelerates() {
        val d = EncoderDrag()
        // 0.5 dp/ms: gain 1 + 4 * 0.25 = 2 -> 40 dp gives 20 steps
        assertEquals(20, (1..5).sumOf { d.move(8f, 16) })
    }

    @Test fun directionChangeDropsLeftoverFraction() {
        val d = EncoderDrag()
        assertEquals(0, d.move(3f, 16))  // 0.75 step carried
        assertEquals(0, d.move(-3f, 16)) // reversal starts fresh: -0.75, not 0
        assertEquals(-1, d.move(-1f, 16))
    }
}
