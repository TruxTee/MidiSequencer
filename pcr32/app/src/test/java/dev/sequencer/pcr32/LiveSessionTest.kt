package dev.sequencer.pcr32

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
        Thread.sleep(500)
        got.clear()
        s.send(Pcr32Map.cc(Pcr32Map.TRANSPORT + 3, 127)); s.send(Pcr32Map.cc(Pcr32Map.TRANSPORT + 3, 0))
        Thread.sleep(300)
        s.send(Pcr32Map.turn(0, 7))
        Thread.sleep(300)
        s.send(Pcr32Map.cc(Pcr32Map.TRANSPORT + 3, 127)); s.send(Pcr32Map.cc(Pcr32Map.TRANSPORT + 3, 0))
        Thread.sleep(500)
        s.close()
        val fb = got.map { m -> m.map { it.toInt() and 0xFF } }
        println("feedback: $fb")
        assertTrue("start light on", fb.contains(listOf(0xBF, 88, 127)))
        assertTrue("ring 1 = 67", fb.contains(listOf(0xBF, 1, 67)))
        assertTrue("start light off", fb.contains(listOf(0xBF, 88, 0)))
    }
}
