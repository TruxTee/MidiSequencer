package dev.sequencer.pcr32

/**
 * PCR32 MIDI map; must match profiles/pcr32.json in the engine.
 * All controls are CC on channel 16. Feedback comes back on the same CC.
 */
object Pcr32Map {
    const val CHANNEL = 16
    const val STATUS = 0xB0 or (CHANNEL - 1)
    const val ENCODER_TURN = 1   // 32 encoders, relative two's complement
    const val ENCODER_PUSH = 33  // 32
    const val PARAM_BUTTON = 65  // 16 (B5..B20)
    const val TRACK_BUTTON = 81  // 4
    const val TRANSPORT = 85     // rec, mixer, reset, start/stop

    val PARAM_LABELS = listOf(
        "PITCH", "VELOCITY", "MUTE", "DELAY", "LENGTH", "CTRL α", "CTRL β", "WAVE GEN",
        "CHORD", "STEP SKIP", "SEQ START", "SEQ LEN", "SEQ OFFS", "CLK DIV", "SHUFFLE", "DIRECTION",
    )
    val TRANSPORT_LABELS = listOf("REC", "MIXER", "RESET", "START/STOP")

    fun cc(cc: Int, value: Int) = byteArrayOf(STATUS.toByte(), cc.toByte(), value.coerceIn(0, 127).toByte())

    /** Relative two's complement: +n -> n, -n -> 128 - n. Steps are limited to ±63. */
    fun turn(encoder: Int, steps: Int): ByteArray {
        val s = steps.coerceIn(-63, 63)
        return cc(ENCODER_TURN + encoder, if (s >= 0) s else 128 + s)
    }
}

/** What the engine has told us to show. Updated from feedback CCs. */
class SurfaceState {
    val ring = IntArray(32)
    val encoderLed = BooleanArray(32)
    val paramLed = BooleanArray(16)
    val trackLed = BooleanArray(4)
    val transportLed = BooleanArray(4)

    /** Applies one received MIDI message. Returns false if it is not PCR32 feedback. */
    fun apply(m: ByteArray): Boolean {
        if (m.size != 3 || (m[0].toInt() and 0xFF) != Pcr32Map.STATUS) return false
        val cc = m[1].toInt() and 0x7F
        val v = m[2].toInt() and 0x7F
        val on = v >= 64
        when (cc) {
            in Pcr32Map.ENCODER_TURN until Pcr32Map.ENCODER_TURN + 32 -> ring[cc - Pcr32Map.ENCODER_TURN] = v
            in Pcr32Map.ENCODER_PUSH until Pcr32Map.ENCODER_PUSH + 32 -> encoderLed[cc - Pcr32Map.ENCODER_PUSH] = on
            in Pcr32Map.PARAM_BUTTON until Pcr32Map.PARAM_BUTTON + 16 -> paramLed[cc - Pcr32Map.PARAM_BUTTON] = on
            in Pcr32Map.TRACK_BUTTON until Pcr32Map.TRACK_BUTTON + 4 -> trackLed[cc - Pcr32Map.TRACK_BUTTON] = on
            in Pcr32Map.TRANSPORT until Pcr32Map.TRANSPORT + 4 -> transportLed[cc - Pcr32Map.TRANSPORT] = on
            else -> return false
        }
        return true
    }
}

fun noteName(n: Int): String {
    val names = arrayOf("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
    return names[n % 12] + (n / 12 - 1)
}
