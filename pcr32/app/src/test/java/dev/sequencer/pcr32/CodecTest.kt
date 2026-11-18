package dev.sequencer.pcr32

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

private fun bytes(vararg v: Int) = ByteArray(v.size) { v[it].toByte() }
private fun rtpHeader() = bytes(0x80, 0x61, 0, 1, 0, 0, 0, 0, 1, 2, 3, 4)

class CodecTest {
    @Test fun invitationLayout() {
        val p = RtpCodec.exchange(RtpCodec.IN, 0x11223344, 0x55667788, "AB")
        assertArrayEquals(
            bytes(0xFF, 0xFF, 'I'.code, 'N'.code, 0, 0, 0, 2, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 'A'.code, 'B'.code, 0),
            p,
        )
    }

    @Test fun clockLayoutIs36Bytes() {
        val p = RtpCodec.clock(1, 2, 3, 4, 5)
        assertEquals(36, p.size)
        assertEquals(2, p[8].toInt())
        assertEquals(5, p[35].toInt())
    }

    @Test fun outgoingMidiPacket() {
        val p = RtpCodec.rtpMidi(0x0102, 0x0A0B0C0D, 0x01020304, bytes(0xBF, 88, 127))
        assertArrayEquals(bytes(0x80, 0x61, 1, 2, 0x0A, 0x0B, 0x0C, 0x0D, 1, 2, 3, 4, 3, 0xBF, 88, 127), p)
    }

    @Test fun parsesRunningStatusAndDeltaTimes() {
        // list: CC(1,60) delta0 [running] (2,61) delta 0x81 0x00 NoteOn(60,100)
        val list = bytes(0xBF, 1, 60, 0x00, 2, 61, 0x81, 0x00, 0x90, 60, 100)
        val p = rtpHeader() + bytes(list.size) + list
        val m = RtpCodec.parseMidi(p, p.size)
        assertEquals(3, m.size)
        assertArrayEquals(bytes(0xBF, 1, 60), m[0])
        assertArrayEquals(bytes(0xBF, 2, 61), m[1])
        assertArrayEquals(bytes(0x90, 60, 100), m[2])
    }

    @Test fun parsesFirstDeltaFlagLongHeaderAndSkipsJournal() {
        val list = bytes(0x05, 0xBF, 33, 127)
        val p = rtpHeader() + bytes(0x80 or 0x40 or 0x20, list.size) + list + bytes(0xAA, 0xBB, 0xCC)
        val m = RtpCodec.parseMidi(p, p.size)
        assertEquals(1, m.size)
        assertArrayEquals(bytes(0xBF, 33, 127), m[0])
    }

    @Test fun skipsSysexAndKeepsRealtime() {
        val list = bytes(0xF0, 0x7D, 1, 2, 0xF7, 0x00, 0xF8, 0x00, 0xBF, 3, 9)
        val p = rtpHeader() + bytes(list.size) + list
        val m = RtpCodec.parseMidi(p, p.size)
        assertEquals(listOf(listOf(0xF8), listOf(0xBF, 3, 9)), m.map { a -> a.map { it.toInt() and 0xFF } })
    }

    @Test fun truncatedPacketsDoNotThrow() {
        val p = rtpHeader() + bytes(5, 0xBF, 1)
        assertEquals(0, RtpCodec.parseMidi(p, p.size).size)
        assertEquals(0, RtpCodec.parseMidi(bytes(1, 2), 2).size)
    }

    @Test fun turnEncodesTwosComplement() {
        assertArrayEquals(bytes(0xBF, 1, 1), Pcr32Map.turn(0, 1))
        assertArrayEquals(bytes(0xBF, 32, 127), Pcr32Map.turn(31, -1))
        assertArrayEquals(bytes(0xBF, 5, 125), Pcr32Map.turn(4, -3))
        assertArrayEquals(bytes(0xBF, 1, 65), Pcr32Map.turn(0, -100)) // clamped to -63
    }

    @Test fun feedbackUpdatesState() {
        val s = SurfaceState()
        assertTrue(s.apply(bytes(0xBF, 3, 64)))
        assertTrue(s.apply(bytes(0xBF, 33, 127)))
        assertTrue(s.apply(bytes(0xBF, 82, 127)))
        assertTrue(s.apply(bytes(0xBF, 88, 127)))
        assertEquals(64, s.ring[2])
        assertTrue(s.encoderLed[0])
        assertTrue(s.trackLed[1])
        assertTrue(s.transportLed[3])
        assertFalse(s.apply(bytes(0xB0, 3, 64))) // wrong channel
        assertFalse(s.apply(bytes(0xBF, 100, 1))) // unmapped
    }

    @Test fun tempoTurnUsesCc91() {
        assertArrayEquals(bytes(0xBF, 91, 3), Pcr32Map.tempo(3))
        assertArrayEquals(bytes(0xBF, 91, 126), Pcr32Map.tempo(-2))
    }

    @Test fun tempoFeedbackIsPitchBendOnChannel16() {
        val s = SurfaceState()
        assertTrue(s.apply(bytes(0xEF, 2, 1))) // 1*128 + 2
        assertEquals(130, s.bpm)
        assertFalse(s.apply(bytes(0xE0, 5, 0))) // other channel
        assertEquals(130, s.bpm)
        val p = rtpHeader() + bytes(3, 0xEF, 20, 0) // survives the RTP parser
        assertArrayEquals(bytes(0xEF, 20, 0), RtpCodec.parseMidi(p, p.size).single())
    }

    @Test fun noteNames() {
        assertEquals("C4", noteName(60))
        assertEquals("C#-1", noteName(1))
        assertEquals("G9", noteName(127))
    }
}
