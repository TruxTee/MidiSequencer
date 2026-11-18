package dev.sequencer.pcr32

import kotlin.math.abs
import kotlin.math.min

/**
 * Turns vertical drag into encoder steps with acceleration: slow drags move one step per
 * [detentDp], fast flicks multiply that by up to [maxGain]. Fractions carry over between moves.
 */
class EncoderDrag(
    private val detentDp: Float = 4f,
    private val slowDpPerMs: Float = 0.25f,
    private val gainPerDpPerMs: Float = 4f,
    private val maxGain: Float = 8f,
) {
    private var carry = 0f

    /** [dyDp] positive = upward drag. Returns whole steps to send now. */
    fun move(dyDp: Float, dtMs: Long): Int {
        if (dyDp == 0f) return 0
        if (carry != 0f && (carry > 0) != (dyDp > 0)) carry = 0f // direction change
        val speed = abs(dyDp) / maxOf(dtMs, 1L)
        val gain = min(maxGain, 1f + gainPerDpPerMs * maxOf(0f, speed - slowDpPerMs))
        carry += dyDp / detentDp * gain
        val steps = carry.toInt()
        carry -= steps
        return steps
    }
}
