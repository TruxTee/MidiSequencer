package dev.sequencer.pcr32

import android.app.Activity
import android.app.AlertDialog
import android.net.wifi.WifiManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.EditText
import java.util.concurrent.Executors

class MainActivity : Activity() {
    private val ui = Handler(Looper.getMainLooper())
    // Network calls are not allowed on the UI thread; one thread keeps messages in order.
    private val net = Executors.newSingleThreadExecutor()
    private val state = SurfaceState()
    private lateinit var view: Pcr32View
    private lateinit var session: RtpMidiSession
    // Without this Android's Wi-Fi power saving delays sporadic packets by up to ~200 ms.
    private val wifiLock by lazy {
        (applicationContext.getSystemService(WIFI_SERVICE) as WifiManager)
            .createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY, "pcr32").apply { setReferenceCounted(false) }
    }

    override fun onResume() {
        super.onResume()
        wifiLock.acquire()
    }

    override fun onPause() {
        wifiLock.release()
        super.onPause()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        session = RtpMidiSession(
            name = "PCR32",
            onMidi = { m -> ui.post { if (state.apply(m)) view.invalidate() } },
            onState = { s -> ui.post { view.status = s } },
        )
        view = Pcr32View(this, state, send = { m -> net.execute { session.send(m) } }, onDisplayTap = ::askHost)
        setContentView(view)
        hideSystemBars()
        connect()
    }

    private fun hideSystemBars() {
        window.insetsController?.let {
            it.hide(WindowInsets.Type.systemBars())
            it.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    private val prefs get() = getSharedPreferences("pcr32", MODE_PRIVATE)

    private fun connect() {
        val host = prefs.getString("host", "") ?: ""
        val port = prefs.getInt("port", 5004)
        if (host.isBlank()) {
            view.status = "not connected — tap display"
            return
        }
        view.status = "connecting to $host:$port …"
        net.execute {
            try {
                session.connect(host, port)
            } catch (e: Exception) {
                ui.post { view.status = "error: ${e.message}" }
            }
        }
    }

    private fun askHost() {
        val input = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
            setText(prefs.getString("host", "")?.let { h -> if (h.isBlank()) "" else "$h:${prefs.getInt("port", 5004)}" })
            hint = "192.168.0.10:5004"
        }
        AlertDialog.Builder(this)
            .setTitle("RTP-MIDI host")
            .setView(input)
            .setPositiveButton("Connect") { _, _ ->
                val parts = input.text.toString().trim().split(":")
                prefs.edit()
                    .putString("host", parts[0])
                    .putInt("port", parts.getOrNull(1)?.toIntOrNull() ?: 5004)
                    .apply()
                connect()
            }
            .setNegativeButton("Cancel", null)
            .setOnDismissListener { hideSystemBars() }
            .show()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    override fun onDestroy() {
        net.execute { session.close() }
        net.shutdown()
        super.onDestroy()
    }
}
