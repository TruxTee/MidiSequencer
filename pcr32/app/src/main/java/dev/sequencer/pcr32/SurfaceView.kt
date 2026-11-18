package dev.sequencer.pcr32

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.view.MotionEvent
import android.view.View
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin

/** Draws the BCR32-style panel and turns touches into PCR32 MIDI messages. */
class Pcr32View(
    context: Context,
    private val state: SurfaceState,
    private val send: (ByteArray) -> Unit,
    private val onDisplayTap: () -> Unit,
) : View(context) {

    private enum class Kind { ENCODER, PARAM, TRACK, TRANSPORT, DISPLAY }
    private class Ctl(val kind: Kind, val index: Int, val rect: RectF)

    private val controls = ArrayList<Ctl>()
    private val dp = resources.displayMetrics.density

    var status = "not connected — tap display"
        set(v) { field = v; invalidate() }
    private var lastEncoder = -1

    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val text = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.WHITE; textAlign = Paint.Align.CENTER }

    private class Touch(val ctl: Ctl, var lastY: Float, var lastAt: Long) {
        val downAt = lastAt
        val drag = EncoderDrag()
        var moved = false
    }
    private val touches = HashMap<Int, Touch>()

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        controls.clear()
        val m = h * 0.02f
        val leftW = w * 0.76f
        val btnH = h * 0.085f
        val colW = (leftW - m) / 8
        for (row in 0..1) for (col in 0..7) {
            val x = m + col * colW
            val y = m + row * (btnH + m)
            controls += Ctl(Kind.PARAM, row * 8 + col, RectF(x, y, x + colW - m, y + btnH))
        }
        val encTop = m + 2 * (btnH + m)
        val rowH = (h - encTop - m) / 4
        for (row in 0..3) for (col in 0..7) {
            val x = m + col * colW
            val y = encTop + row * rowH
            controls += Ctl(Kind.ENCODER, row * 8 + col, RectF(x, y, x + colW - m, y + rowH - m))
        }
        val rx = leftW + m
        val rw = w - rx - m
        controls += Ctl(Kind.DISPLAY, 0, RectF(rx, m, rx + rw, m + h * 0.22f))
        val gridTop = m + h * 0.22f + m
        val cellH = (h - gridTop - m) / 4
        val cellW = (rw - m) / 2
        for (i in 0..3) {
            val x = rx + (i % 2) * (cellW + m)
            val y = gridTop + (i / 2) * cellH
            controls += Ctl(Kind.TRACK, i, RectF(x, y, x + cellW, y + cellH - m))
        }
        for (i in 0..3) {
            val x = rx + (i % 2) * (cellW + m)
            val y = gridTop + (2 + i / 2) * cellH
            controls += Ctl(Kind.TRANSPORT, i, RectF(x, y, x + cellW, y + cellH - m))
        }
    }

    override fun onDraw(c: Canvas) {
        c.drawColor(Color.rgb(18, 18, 20))
        for (ctl in controls) when (ctl.kind) {
            Kind.ENCODER -> drawEncoder(c, ctl)
            Kind.PARAM -> drawButton(c, ctl.rect, Pcr32Map.PARAM_LABELS[ctl.index], state.paramLed[ctl.index], Color.rgb(255, 140, 0))
            Kind.TRACK -> drawButton(c, ctl.rect, "T${ctl.index + 1}", state.trackLed[ctl.index], Color.rgb(0, 170, 255))
            Kind.TRANSPORT -> drawButton(c, ctl.rect, Pcr32Map.TRANSPORT_LABELS[ctl.index], state.transportLed[ctl.index], Color.rgb(40, 200, 80))
            Kind.DISPLAY -> drawDisplay(c, ctl.rect)
        }
    }

    private fun drawButton(c: Canvas, r: RectF, label: String, lit: Boolean, litColor: Int) {
        fill.color = if (lit) litColor else Color.rgb(55, 55, 60)
        c.drawRoundRect(r, 6 * dp, 6 * dp, fill)
        text.color = if (lit) Color.BLACK else Color.LTGRAY
        text.textSize = min(r.height() * 0.38f, r.width() / (label.length * 0.62f + 1))
        c.drawText(label, r.centerX(), r.centerY() + text.textSize / 3, text)
    }

    private fun drawEncoder(c: Canvas, ctl: Ctl) {
        val r = ctl.rect
        val radius = min(r.width(), r.height()) * 0.42f
        val cx = r.centerX(); val cy = r.centerY()
        // 15-LED ring over 270 degrees, lit as a bar up to the value.
        val lit = state.ring[ctl.index] * 15 / 127
        for (i in 0 until 15) {
            val a = Math.toRadians(135.0 + i * 270.0 / 14)
            fill.color = if (i <= lit && state.ring[ctl.index] > 0) Color.rgb(255, 60, 40) else Color.rgb(60, 25, 25)
            c.drawCircle(cx + (radius * cos(a)).toFloat(), cy + (radius * sin(a)).toFloat(), radius * 0.09f, fill)
        }
        fill.color = Color.rgb(45, 45, 50)
        c.drawCircle(cx, cy, radius * 0.72f, fill)
        fill.color = if (state.encoderLed[ctl.index]) Color.rgb(255, 140, 0) else Color.rgb(70, 70, 75)
        c.drawCircle(cx, cy, radius * 0.16f, fill)
        text.color = Color.GRAY
        text.textSize = radius * 0.32f
        c.drawText("${ctl.index + 1}", cx, cy + radius * 0.62f, text)
    }

    private fun drawDisplay(c: Canvas, r: RectF) {
        fill.color = Color.BLACK
        c.drawRect(r, fill)
        text.color = Color.rgb(120, 200, 255)
        text.textSize = r.height() * 0.22f
        val track = state.trackLed.indexOfFirst { it } + 1
        val line1 = if (lastEncoder >= 0) "T$track  S${lastEncoder + 1}  ${noteName(state.ring[lastEncoder])}" else "PCR32"
        c.drawText(line1, r.centerX(), r.top + r.height() * 0.42f, text)
        text.textSize = r.height() * 0.13f
        text.color = Color.GRAY
        c.drawText(status, r.centerX(), r.top + r.height() * 0.8f, text)
    }

    private fun hit(x: Float, y: Float) = controls.firstOrNull { it.rect.contains(x, y) }

    private fun press(ctl: Ctl, down: Boolean) {
        val base = when (ctl.kind) {
            Kind.PARAM -> Pcr32Map.PARAM_BUTTON
            Kind.TRACK -> Pcr32Map.TRACK_BUTTON
            Kind.TRANSPORT -> Pcr32Map.TRANSPORT
            else -> return
        }
        send(Pcr32Map.cc(base + ctl.index, if (down) 127 else 0))
    }

    override fun onTouchEvent(e: MotionEvent): Boolean {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                val i = e.actionIndex
                val ctl = hit(e.getX(i), e.getY(i)) ?: return true
                if (ctl.kind == Kind.DISPLAY) { onDisplayTap(); return true }
                touches[e.getPointerId(i)] = Touch(ctl, e.getY(i), e.eventTime)
                if (ctl.kind == Kind.ENCODER) lastEncoder = ctl.index else press(ctl, true)
                invalidate()
            }
            MotionEvent.ACTION_MOVE -> for (i in 0 until e.pointerCount) {
                val t = touches[e.getPointerId(i)] ?: continue
                if (t.ctl.kind != Kind.ENCODER) continue
                val y = e.getY(i)
                var steps = t.drag.move((t.lastY - y) / dp, e.eventTime - t.lastAt)
                t.lastY = y
                t.lastAt = e.eventTime
                if (steps != 0) {
                    t.moved = true
                    // Show the turn at once; the engine's feedback then sets the real value.
                    state.ring[t.ctl.index] = (state.ring[t.ctl.index] + steps).coerceIn(0, 127)
                    while (steps != 0) { // one message carries at most ±63
                        val chunk = steps.coerceIn(-63, 63)
                        send(Pcr32Map.turn(t.ctl.index, chunk))
                        steps -= chunk
                    }
                    invalidate()
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_CANCEL -> {
                val id = e.getPointerId(e.actionIndex)
                val t = touches.remove(id) ?: return true
                if (t.ctl.kind == Kind.ENCODER) {
                    // A short touch without turning is a push.
                    if (!t.moved && e.actionMasked != MotionEvent.ACTION_CANCEL && e.eventTime - t.downAt < 400) {
                        send(Pcr32Map.cc(Pcr32Map.ENCODER_PUSH + t.ctl.index, 127))
                        send(Pcr32Map.cc(Pcr32Map.ENCODER_PUSH + t.ctl.index, 0))
                    }
                } else {
                    press(t.ctl, false)
                }
                if (e.actionMasked == MotionEvent.ACTION_CANCEL) touches.clear()
            }
        }
        return true
    }
}
