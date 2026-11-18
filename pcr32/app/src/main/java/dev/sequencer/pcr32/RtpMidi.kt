package dev.sequencer.pcr32

import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.SocketTimeoutException
import java.nio.ByteBuffer
import kotlin.random.Random

/** Byte layouts of AppleMIDI session packets and RTP-MIDI payloads (RFC 6295). Pure: no I/O. */
object RtpCodec {
    const val PROTOCOL_VERSION = 2
    private const val SIG = 0xFFFF.toShort()

    fun cmd(a: Char, b: Char): Short = ((a.code shl 8) or b.code).toShort()
    val IN = cmd('I', 'N')
    val OK = cmd('O', 'K')
    val NO = cmd('N', 'O')
    val BY = cmd('B', 'Y')
    val CK = cmd('C', 'K')

    fun exchange(command: Short, token: Int, ssrc: Int, name: String? = null): ByteArray {
        val n = name?.toByteArray(Charsets.UTF_8)
        val b = ByteBuffer.allocate(16 + (n?.size?.plus(1) ?: 0))
        b.putShort(SIG).putShort(command).putInt(PROTOCOL_VERSION).putInt(token).putInt(ssrc)
        if (n != null) b.put(n).put(0)
        return b.array()
    }

    fun clock(ssrc: Int, count: Int, t1: Long, t2: Long, t3: Long): ByteArray =
        ByteBuffer.allocate(36).putShort(SIG).putShort(CK).putInt(ssrc).put(count.toByte())
            .put(0).put(0).put(0).putLong(t1).putLong(t2).putLong(t3).array()

    /** Command of an AppleMIDI session packet, or null if [p] is not one. */
    fun sessionCommand(p: ByteArray, len: Int): Short? =
        if (len >= 4 && p[0] == 0xFF.toByte() && p[1] == 0xFF.toByte()) ByteBuffer.wrap(p, 2, 2).short else null

    /** One MIDI message in an RTP-MIDI packet; no journal, no delta times. */
    fun rtpMidi(seq: Int, timestamp: Int, ssrc: Int, msg: ByteArray): ByteArray {
        require(msg.size in 1..15)
        return ByteBuffer.allocate(13 + msg.size)
            .put(0x80.toByte()).put(0x61).putShort(seq.toShort()).putInt(timestamp).putInt(ssrc)
            .put(msg.size.toByte()).put(msg).array()
    }

    private fun dataLength(status: Int): Int = when (status and 0xF0) {
        0x80, 0x90, 0xA0, 0xB0, 0xE0 -> 2
        0xC0, 0xD0 -> 1
        else -> when (status) { 0xF1, 0xF3 -> 1; 0xF2 -> 2; else -> 0 }
    }

    /**
     * Channel and system messages from an RTP-MIDI packet. Handles delta times, running status,
     * the long-header flag and a trailing journal (skipped). SysEx is skipped.
     */
    fun parseMidi(p: ByteArray, len: Int): List<ByteArray> {
        val out = ArrayList<ByteArray>()
        if (len < 13 || (p[0].toInt() and 0xC0) != 0x80) return out
        var i = 12
        val h = p[i].toInt() and 0xFF
        val long = h and 0x80 != 0
        val listLen = if (long) ((h and 0x0F) shl 8) or (p[i + 1].toInt() and 0xFF) else h and 0x0F
        i += if (long) 2 else 1
        val end = minOf(len, i + listLen)
        var first = true
        val hasFirstDelta = h and 0x20 != 0
        var running = 0
        while (i < end) {
            if (!first || hasFirstDelta) {
                var k = 0
                while (i < end && k < 4 && (p[i].toInt() and 0x80) != 0) { i++; k++ }
                i++ // last delta byte
                if (i >= end) break
            }
            first = false
            val b = p[i].toInt() and 0xFF
            val status: Int
            if (b and 0x80 != 0) {
                status = b; i++
                if (b == 0xF0) { // SysEx: skip to F7 or end of list
                    while (i < end && (p[i].toInt() and 0xFF) != 0xF7) i++
                    i++
                    continue
                }
                if (b < 0xF0) running = b else if (b < 0xF8) running = 0
            } else {
                if (running == 0) break
                status = running
            }
            val n = dataLength(status)
            if (i + n > end) break
            val m = ByteArray(1 + n)
            m[0] = status.toByte()
            System.arraycopy(p, i, m, 1, n)
            out.add(m)
            i += n
        }
        return out
    }
}

/** A session is alive while the host has been heard from within [timeoutNs]. */
class Liveness(private val timeoutNs: Long) {
    @Volatile private var lastNs: Long? = null
    fun heard(nowNs: Long) { lastNs = nowNs }
    fun reset() { lastNs = null }
    fun alive(nowNs: Long): Boolean = lastNs?.let { nowNs - it < timeoutNs } ?: false
}

/**
 * AppleMIDI session as initiator: invites a host (rtpMIDI on Windows, rtpmidid on the Pi),
 * keeps clock sync running, sends MIDI and delivers received MIDI to [onMidi].
 */
class RtpMidiSession(
    private val name: String,
    private val onMidi: (ByteArray) -> Unit,
    private val onState: (String) -> Unit,
) {
    private val ssrc = Random.nextInt()
    private val t0 = System.nanoTime()
    private var control: DatagramSocket? = null
    private var data: DatagramSocket? = null
    private var host: InetSocketAddress? = null
    @Volatile private var running = false
    // Each connect starts a new generation; loops of an older one see the change and end.
    @Volatile private var gen = 0
    private val liveness = Liveness(5_000_000_000L)
    private var seq = Random.nextInt(0, 0xFFFF)

    private fun now100us(): Long = (System.nanoTime() - t0) / 100_000

    /** Blocks until the host accepts both ports or the attempt fails. Call off the UI thread. */
    fun connect(hostName: String, port: Int): Boolean {
        close()
        val addr = InetAddress.getByName(hostName)
        val c = DatagramSocket(0)
        val d = bindDataNextTo(c) ?: run { c.close(); onState("no local ports"); return false }
        control = c; data = d
        val token = Random.nextInt()
        if (!invite(c, InetSocketAddress(addr, port), token) || !invite(d, InetSocketAddress(addr, port + 1), token)) {
            onState("host refused or not reachable")
            close()
            return false
        }
        host = InetSocketAddress(addr, port)
        running = true
        liveness.heard(System.nanoTime())
        val g = ++gen
        Thread({ receiveLoop(d, g) }, "rtp-data").apply { isDaemon = true }.start()
        Thread({ receiveLoop(c, g) }, "rtp-control").apply { isDaemon = true }.start()
        Thread({ syncLoop(g) }, "rtp-sync").apply { isDaemon = true }.start()
        onState("connected to $hostName")
        return true
    }

    // AppleMIDI expects the data port to be the control port + 1.
    private fun bindDataNextTo(c: DatagramSocket): DatagramSocket? = try {
        DatagramSocket(c.localPort + 1)
    } catch (e: Exception) {
        null
    }

    private fun invite(s: DatagramSocket, to: InetSocketAddress, token: Int): Boolean {
        val pkt = RtpCodec.exchange(RtpCodec.IN, token, ssrc, name)
        val buf = ByteArray(512)
        s.soTimeout = 1000
        repeat(5) {
            s.send(DatagramPacket(pkt, pkt.size, to))
            try {
                val r = DatagramPacket(buf, buf.size)
                s.receive(r)
                when (RtpCodec.sessionCommand(buf, r.length)) {
                    RtpCodec.OK -> return true
                    RtpCodec.NO -> return false
                }
            } catch (_: SocketTimeoutException) {
            }
        }
        return false
    }

    /** True while the host keeps answering; false after 5 s of silence, a BY or close(). */
    fun isAlive(): Boolean = running && liveness.alive(System.nanoTime())

    private fun receiveLoop(s: DatagramSocket, g: Int) {
        val buf = ByteArray(2048)
        s.soTimeout = 0
        while (running && gen == g) {
            val r = DatagramPacket(buf, buf.size)
            try { s.receive(r) } catch (_: Exception) { break }
            liveness.heard(System.nanoTime())
            when (RtpCodec.sessionCommand(buf, r.length)) {
                RtpCodec.CK -> answerClock(s, buf, r)
                RtpCodec.BY -> { onState("host ended session"); running = false }
                null -> RtpCodec.parseMidi(buf, r.length).forEach(onMidi)
            }
        }
    }

    private fun answerClock(s: DatagramSocket, buf: ByteArray, r: DatagramPacket) {
        if (r.length < 36) return
        val b = ByteBuffer.wrap(buf, 0, 36)
        val count = buf[8].toInt()
        val t1 = b.getLong(12); val t2 = b.getLong(20)
        val reply = when (count) {
            0 -> RtpCodec.clock(ssrc, 1, t1, now100us(), 0)
            1 -> RtpCodec.clock(ssrc, 2, t1, t2, now100us())
            else -> return
        }
        s.send(DatagramPacket(reply, reply.size, r.socketAddress))
    }

    // Initial burst, then every 2 s: hosts drop sessions without clock sync, and each answer
    // tells [isAlive] the host is still there.
    private fun syncLoop(g: Int) {
        var n = 0
        while (running && gen == g) {
            val d = data ?: break
            val h = host ?: break
            val pkt = RtpCodec.clock(ssrc, 0, now100us(), 0, 0)
            try { d.send(DatagramPacket(pkt, pkt.size, InetSocketAddress(h.address, h.port + 1))) } catch (_: Exception) {}
            Thread.sleep(if (n++ < 6) 1500 else 2000)
        }
    }

    /** Sends one MIDI message. Safe from any non-UI thread. */
    @Synchronized
    fun send(msg: ByteArray) {
        val d = data ?: return
        val h = host ?: return
        seq = (seq + 1) and 0xFFFF
        val pkt = RtpCodec.rtpMidi(seq, now100us().toInt(), ssrc, msg)
        try { d.send(DatagramPacket(pkt, pkt.size, InetSocketAddress(h.address, h.port + 1))) } catch (_: Exception) {}
    }

    fun close() {
        val h = host
        val c = control
        if (running && h != null && c != null) {
            val by = RtpCodec.exchange(RtpCodec.BY, 0, ssrc)
            try { c.send(DatagramPacket(by, by.size, h)) } catch (_: Exception) {}
        }
        running = false
        liveness.reset()
        control?.close(); data?.close()
        control = null; data = null; host = null
    }
}
