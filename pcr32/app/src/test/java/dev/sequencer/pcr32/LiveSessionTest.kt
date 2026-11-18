package dev.sequencer.pcr32

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.util.concurrent.CopyOnWriteArrayList

/**
 * Talks to a real RTP-MIDI host with the engine running behind it.
 * Run: gradlew test -Drtp.host=127.0.0.1 --tests '*LiveSessionTest*'
 */
class LiveSessionTest {
    @Test fun pressStartTurnEncoderAndReceiveFeedback() {
        val host = System.getProperty("rtp.host").orEmpty()
        assumeTrue("set -Drtp.host to run", host.isNotBlank())
        val got = CopyOnWriteArrayList<ByteArray>()
        val s = RtpMidiSession("PCR32 test", onMidi = { got.add(it) }, onState = { println("state: $it") })
        assertTrue("session refused", s.connect(host, 5004))
        Thread.sleep(3000) // the engine finds the new port and sends the whole surface
        val ring1 = { got.map { m -> m.map { it.toInt() and 0xFF } }.lastOrNull { it.take(2) == listOf(0xBF, 1) }?.get(2) }
        val before = ring1()
        assertNotNull("no surface refresh", before)
        got.clear()
        s.send(Pcr32Map.cc(Pcr32Map.TRANSPORT + 3, 127)); s.send(Pcr32Map.cc(Pcr32Map.TRANSPORT + 3, 0))
        Thread.sleep(300)
        s.send(Pcr32Map.turn(0, 7))
        Thread.sleep(300)
        val after = ring1()
        s.send(Pcr32Map.turn(0, -7)) // leave the pattern as it was
        s.send(Pcr32Map.cc(Pcr32Map.TRANSPORT + 3, 127)); s.send(Pcr32Map.cc(Pcr32Map.TRANSPORT + 3, 0))
        Thread.sleep(500)
        s.close()
        val fb = got.map { m -> m.map { it.toInt() and 0xFF } }
        assertTrue("start light on", fb.contains(listOf(0xBF, 88, 127)))
        assertEquals("ring 1 moved by 7", before!! + 7, after)
        assertTrue("start light off", fb.contains(listOf(0xBF, 88, 0)))
    }

    @Test fun staysAliveWhileHostAnswers() {
        val host = System.getProperty("rtp.host").orEmpty()
        assumeTrue("set -Drtp.host to run", host.isNotBlank())
        // Not "PCR32": the engine must not take this session for the controller.
        val s = RtpMidiSession("LIVENESS test", onMidi = {}, onState = { println("state: $it") })
        assertTrue("session refused", s.connect(host, 5004))
        for (i in 1..8) {
            Thread.sleep(1000)
            println("t=${i}s alive=${s.isAlive()}")
        }
        val alive = s.isAlive()
        s.close()
        assertTrue("host went silent", alive)
    }

    // The phone app reuses one session object: the reconnected session must stay alive too.
    @Test fun staysAliveAfterReconnectWithSameObject() {
        val host = System.getProperty("rtp.host").orEmpty()
        assumeTrue("set -Drtp.host to run", host.isNotBlank())
        val s = RtpMidiSession("LIVENESS test", onMidi = {}, onState = { println("state: $it") })
        assertTrue("first session refused", s.connect(host, 5004))
        Thread.sleep(3000)
        assertTrue("second session refused", s.connect(host, 5004))
        Thread.sleep(7000)
        val alive = s.isAlive()
        s.close()
        assertTrue("reconnected session went silent", alive)
    }
}
