package dev.farhanlabib.clipcircle

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.text.InputType
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.WindowInsets
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
    private lateinit var autoSwitch: Switch
    private lateinit var autoStatus: TextView
    private lateinit var autoSteps: LinearLayout
    private lateinit var overlayButton: Button
    private lateinit var logRow: LinearLayout

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
        refreshAutoSend()
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

    private fun refreshAutoSend() {
        val enabled = AutoSend.enabled(this)
        val overlay = AutoSend.canOverlay(this)
        val log = AutoSend.canReadLog(this)
        autoSwitch.setOnCheckedChangeListener(null)
        autoSwitch.isChecked = enabled
        autoSwitch.setOnCheckedChangeListener { _, on -> if (on) askAutoSend() else setAutoSend(false) }
        autoStatus.text = when {
            !enabled -> "Off. Copies on this phone are sent when you tap Send clipboard."
            overlay && log && SyncService.running -> "On. Whatever you copy on this phone goes to your devices."
            overlay && log -> "Ready. Turn syncing on to start."
            else -> "Two one-time steps left:"
        }
        autoSteps.visibility = if (enabled && !(overlay && log)) View.VISIBLE else View.GONE
        overlayButton.visibility = if (overlay) View.GONE else View.VISIBLE
        logRow.visibility = if (log) View.GONE else View.VISIBLE
        // Starts or stops sending to match.
        if (SyncService.running) SyncService.start(this)
    }

    /** Says what Send automatically needs before turning it on. */
    private fun askAutoSend() {
        AlertDialog.Builder(this)
            .setTitle("Send copies automatically?")
            .setMessage(
                "Android doesn't let apps in the background read the clipboard. To send each copy " +
                    "without a tap, ClipCircle needs two things:\n\n" +
                    "• Permission to read the system log, granted once from a computer. The log can hold " +
                    "details from other apps. ClipCircle only looks for the line saying the clipboard " +
                    "changed, and never saves or sends the log.\n\n" +
                    "• Permission to show over other apps. After each copy ClipCircle takes focus for a " +
                    "moment to read the clipboard, which can close an open keyboard.\n\n" +
                    "You can turn this off any time.",
            )
            .setPositiveButton("Turn on") { _, _ -> setAutoSend(true) }
            .setNegativeButton("Not now") { _, _ -> refreshAutoSend() }
            .setOnCancelListener { refreshAutoSend() }
            .show()
    }

    private fun setAutoSend(on: Boolean) {
        AutoSend.setEnabled(this, on)
        refreshAutoSend()
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
        // Codes are shown as "123 456"; accept them typed or pasted either way.
        val code = joinCode.text.toString().filter { it.isDigit() }
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
        title = text("ClipCircle").apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 22f)
            typeface = Typeface.DEFAULT_BOLD
        }
        syncSwitch = Switch(this)
        header.addView(title, LinearLayout.LayoutParams(0, -2, 1f))
        header.addView(syncSwitch)
        column.addView(header)
        column.addView(text("Clips copied on your other devices land on this phone's clipboard.", muted = true))

        column.addView(section("Send from this phone"))
        column.addView(text("Copy something, then tap below, use the notification's Send clipboard button, or share text to ClipCircle.", muted = true))
        column.addView(Button(this).apply {
            text = "Send clipboard now"
            setOnClickListener { startActivity(Intent(this@MainActivity, SendActivity::class.java)) }
        })

        column.addView(section("Send automatically"))
        val autoRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        autoStatus = text("", muted = true)
        autoSwitch = Switch(this)
        autoRow.addView(autoStatus, LinearLayout.LayoutParams(0, -2, 1f))
        autoRow.addView(autoSwitch)
        column.addView(autoRow)
        autoSteps = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        overlayButton = Button(this).apply {
            text = "1. Allow display over other apps"
            setOnClickListener {
                startActivity(
                    Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:$packageName")),
                )
            }
        }
        autoSteps.addView(overlayButton)
        logRow = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        logRow.addView(text("2. Connect this phone to a computer with USB debugging on and run:", muted = true))
        logRow.addView(text(AutoSend.grantCommand(this)).apply {
            typeface = Typeface.MONOSPACE
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setTextIsSelectable(true)
            setPadding(0, dp(6), 0, dp(6))
        })
        logRow.addView(Button(this, null, android.R.attr.borderlessButtonStyle).apply {
            text = "Copy command"
            setOnClickListener {
                getSystemService(ClipboardManager::class.java)
                    .setPrimaryClip(ClipData.newPlainText("adb command", AutoSend.grantCommand(this@MainActivity)))
            }
        })
        logRow.addView(text("On Android 13 and later, choose Allow when asked about device logs.", muted = true))
        autoSteps.addView(logRow)
        column.addView(autoSteps)

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
            hint = "6-digit code"
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

        return ScrollView(this).apply {
            addView(column)
            // Android 15 draws apps under the status and navigation bars.
            if (Build.VERSION.SDK_INT >= 30) {
                setOnApplyWindowInsetsListener { view, insets ->
                    val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.ime())
                    view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                    insets
                }
            }
        }
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
