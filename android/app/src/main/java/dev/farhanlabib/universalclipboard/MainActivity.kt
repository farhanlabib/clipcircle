package dev.farhanlabib.universalclipboard

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.text.InputType
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView
import uniffi.clip_ffi.Device

/** Pair devices, see the circle, and turn syncing on or off. */
class MainActivity : Activity() {
    private lateinit var title: TextView
    private lateinit var syncSwitch: Switch
    private lateinit var members: LinearLayout
    private lateinit var pairPanel: TextView
    private lateinit var joinCode: EditText
    private lateinit var joinStatus: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildUi())
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }
        // Sync starts on first launch; the switch turns it off.
        if (!SyncService.running) SyncService.start(this)
    }

    override fun onResume() {
        super.onResume()
        clipApp.onPairingResult = { ok, message ->
            pairPanel.text = if (ok) "$message joined your circle." else "Pairing failed: $message"
            refresh()
        }
        refresh()
    }

    override fun onPause() {
        clipApp.onPairingResult = null
        super.onPause()
    }

    private fun refresh() {
        syncSwitch.setOnCheckedChangeListener(null)
        syncSwitch.isChecked = SyncService.running
        syncSwitch.setOnCheckedChangeListener { _, on ->
            if (on) SyncService.start(this) else SyncService.stop(this)
            window.decorView.postDelayed({ refresh() }, 500)
        }
        clipApp.background({ clipApp.node.deviceName() to clipApp.node.members() }) { result ->
            result.onSuccess { (name, list) ->
                title.text = name
                showMembers(list)
            }
        }
    }

    private fun showMembers(list: List<Device>) {
        members.removeAllViews()
        if (list.isEmpty()) {
            members.addView(text("No other devices yet. Add one below.", muted = true))
        }
        for (device in list) {
            val row = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, dp(6), 0, dp(6))
            }
            row.addView(text(device.name), LinearLayout.LayoutParams(0, -2, 1f))
            row.addView(Button(this, null, android.R.attr.borderlessButtonStyle).apply {
                text = "Remove"
                setOnClickListener { confirmRemove(device) }
            })
            members.addView(row)
        }
    }

    private fun confirmRemove(device: Device) {
        AlertDialog.Builder(this)
            .setMessage("Remove ${device.name} from your circle?")
            .setPositiveButton("Remove") { _, _ ->
                clipApp.background({ clipApp.node.removeDevice(device.id) }) { refresh() }
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun startPairing() {
        pairPanel.visibility = View.VISIBLE
        pairPanel.text = "Starting…"
        clipApp.background({ clipApp.node.startPairing() }) { result ->
            pairPanel.text = result.fold(
                { code -> "On the other device, choose Join a circle and type:\n\n${code.chunked(3).joinToString(" ")}\n\nWaiting for the other device…" },
                { "Could not show a code: ${it.message}. Is syncing on?" },
            )
        }
    }

    private fun join() {
        val code = joinCode.text.toString().trim()
        if (!Regex("\\d{6}").matches(code)) {
            joinStatus.text = "The code has 6 digits."
            return
        }
        joinStatus.text = "Looking for the device…"
        clipApp.background({ clipApp.node.join(code) }) { result ->
            joinStatus.text = result.fold(
                { n -> "Joined. $n other device(s) in the circle." },
                { "Could not join: ${it.message}" },
            )
            if (result.isSuccess) joinCode.setText("")
            refresh()
        }
    }

    // --- layout -------------------------------------------------------------

    private fun buildUi(): View {
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(24), dp(20), dp(24))
        }
        val header = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        title = text("Universal Clipboard").apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 22f)
            typeface = Typeface.DEFAULT_BOLD
        }
        syncSwitch = Switch(this)
        header.addView(title, LinearLayout.LayoutParams(0, -2, 1f))
        header.addView(syncSwitch)
        column.addView(header)
        column.addView(text("Clips copied on your other devices land on this phone's clipboard.", muted = true))

        column.addView(section("Send from this phone"))
        column.addView(text("Copy something, then tap below, use the notification's Send clipboard button, or share text to Universal Clipboard.", muted = true))
        column.addView(Button(this).apply {
            text = "Send clipboard now"
            setOnClickListener { startActivity(Intent(this@MainActivity, SendActivity::class.java)) }
        })

        column.addView(section("Devices in your circle"))
        members = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        column.addView(members)
        column.addView(Button(this).apply {
            text = "Add a device"
            setOnClickListener { startPairing() }
        })
        pairPanel = text("").apply {
            visibility = View.GONE
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            setPadding(0, dp(8), 0, dp(8))
        }
        column.addView(pairPanel)

        column.addView(section("Join a circle"))
        column.addView(text("Type the 6-digit code shown on a device in the circle.", muted = true))
        val joinRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        joinCode = EditText(this).apply {
            hint = "123456"
            inputType = InputType.TYPE_CLASS_NUMBER
        }
        joinRow.addView(joinCode, LinearLayout.LayoutParams(0, -2, 1f))
        joinRow.addView(Button(this).apply {
            text = "Join"
            setOnClickListener { join() }
        })
        column.addView(joinRow)
        joinStatus = text("", muted = true)
        column.addView(joinStatus)

        return ScrollView(this).apply { addView(column) }
    }

    private fun section(label: String) = text(label.uppercase()).apply {
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
        typeface = Typeface.DEFAULT_BOLD
        alpha = 0.6f
        setPadding(0, dp(24), 0, dp(6))
    }

    private fun text(value: String, muted: Boolean = false) = TextView(this).apply {
        text = value
        setTextSize(TypedValue.COMPLEX_UNIT_SP, if (muted) 13f else 16f)
        if (muted) alpha = 0.7f
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
}
