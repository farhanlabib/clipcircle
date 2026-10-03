package dev.farhanlabib.clipcircle

import android.Manifest
import android.app.Activity
import android.app.Dialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.graphics.drawable.StateListDrawable
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.text.Editable
import android.text.InputType
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.TextWatcher
import android.text.style.ForegroundColorSpan
import android.text.style.StyleSpan
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import uniffi.clip_ffi.Device
import uniffi.clip_ffi.DeviceCheck
import uniffi.clip_ffi.HistoryItem

/** Pair devices, see the circle and recent clips, and turn syncing on or off. */
class MainActivity : Activity() {
    private lateinit var look: Look
    private val p get() = look.p

    private lateinit var nameView: TextView
    private lateinit var syncLine: TextView
    private lateinit var syncSwitch: M3Switch
    private lateinit var checkButton: ImageButton
    private lateinit var devicesHeading: TextView
    private lateinit var devices: LinearLayout
    private lateinit var sending: LinearLayout
    private lateinit var recent: LinearLayout

    /** Results of the last Check connections, by device id. */
    private var checks: Map<String, DeviceCheck> = emptyMap()
    private var myName = ""
    private var history: List<HistoryItem> = emptyList()
    private var shownDevices: List<DeviceRow>? = null
    private var shownRecent: List<RecentRow>? = null

    /** The Recent row last copied again, and until when it says so. */
    private var copied: Pair<Int, Long>? = null
    private var pairSheet: PairSheet? = null

    private val ticker = Handler(Looper.getMainLooper())
    private val tick = object : Runnable {
        override fun run() {
            load()
            ticker.postDelayed(this, 2000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        look = Look(this)
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
            pairSheet?.finished(ok, message)
            load()
        }
        refreshAutoSend()
        // Starts or stops sending automatically to match the settings.
        if (SyncService.running) SyncService.start(this)
        ticker.post(tick)
    }

    override fun onPause() {
        ticker.removeCallbacks(tick)
        clipApp.onPairingResult = null
        super.onPause()
    }

    // --- data -----------------------------------------------------------------

    private data class DeviceRow(val id: String, val name: String, val online: Boolean, val check: DeviceCheck?)

    private data class RecentRow(val index: Int, val kind: String, val preview: String, val detail: String, val text: String?)

    private fun load() {
        clipApp.background({
            Triple(
                clipApp.node.deviceName(),
                clipApp.node.members(),
                runCatching { clipApp.node.history() }.getOrNull(),
            )
        }) { result ->
            result.onSuccess { (name, members, items) ->
                myName = name
                showHeader(name, members)
                showDevices(members)
                if (items != null) history = items
                showRecent()
            }
        }
    }

    private fun showHeader(name: String, members: List<Device>) {
        nameView.text = name
        val running = SyncService.running
        syncSwitch.setChecked(running)
        val online = members.count { it.online }
        syncLine.text = when {
            !running -> "Off. Nothing is shared with this phone."
            members.isEmpty() -> "Syncing · no other devices yet"
            else -> "Syncing · $online of ${members.size} ${if (members.size == 1) "device" else "devices"} nearby"
        }
    }

    private fun showDevices(members: List<Device>) {
        val rows = members.map { DeviceRow(it.id, it.name, it.online, checks[it.id]) }
        if (rows == shownDevices) return
        shownDevices = rows
        devicesHeading.text = if (rows.isEmpty()) "Devices" else "Devices · ${rows.size}"
        devices.removeAllViews()
        if (rows.isEmpty()) {
            look.addRow(
                devices,
                look.row("No other devices yet", "Tap Add a device, then type the code on your other device."),
            )
        }
        for (device in rows) {
            val check = device.check
            val detail = check?.detail?.takeIf { it.isNotEmpty() }
                ?: if (device.online) "On this network" else "Not on this network"
            val more = look.iconButton(R.drawable.ic_more, "More for ${device.name}") { anchor ->
                PopupMenu(this, anchor).apply {
                    menu.add("Remove from circle").setOnMenuItemClickListener {
                        confirmRemove(device)
                        true
                    }
                    show()
                }
            }
            look.addRow(
                devices,
                look.row(
                    device.name,
                    detail,
                    leading = look.avatar(R.drawable.ic_device, device.online),
                    trailing = more,
                    subtitleColor = if (check != null && !check.ok) p.error else p.onSurfaceVariant,
                ),
            )
        }
    }

    private fun showRecent() {
        val now = SystemClock.elapsedRealtime()
        val copiedIndex = copied?.takeIf { it.second > now }?.first
        val rows = history.take(20).mapIndexed { index, item ->
            val from = if (item.from == myName) "This phone" else "From ${item.from}"
            val detail = if (index == copiedIndex) "Copied" else "$from · ${ago(item.secsAgo.toLong())}"
            RecentRow(index, item.kind, item.preview, detail, item.text)
        }
        if (rows == shownRecent) return
        shownRecent = rows
        recent.removeAllViews()
        if (rows.isEmpty()) {
            look.addRow(recent, look.row("Nothing yet", "Copy something on any of your devices and it shows up here."))
        }
        for (row in rows) {
            val icon = when (row.kind) {
                "link" -> R.drawable.ic_link
                "image" -> R.drawable.ic_image
                "files" -> R.drawable.ic_file
                else -> R.drawable.ic_text
            }
            val view = look.row(row.preview, row.detail, leading = look.leadingIcon(icon), singleLine = true)
            val text = row.text
            if (text != null) {
                view.background = look.ripple(null, 0f)
                view.contentDescription = "${row.preview}. ${row.detail}. Tap to copy again."
                view.setOnClickListener { copyAgain(row.index, text) }
            }
            look.addRow(recent, view)
        }
    }

    private fun copyAgain(index: Int, text: String) {
        // Labelled as the circle's own, so it isn't sent back out.
        clipApp.setOwnClip(ClipData.newPlainText(ClipApp.LABEL_TEXT, text))
        copied = index to SystemClock.elapsedRealtime() + 1500
        showRecent()
        ticker.postDelayed({ showRecent() }, 1600)
    }

    private fun ago(secs: Long) = when {
        secs < 60 -> "just now"
        secs < 3600 -> "${secs / 60} min ago"
        secs < 86400 -> "${secs / 3600} h ago"
        else -> "${secs / 86400} d ago"
    }

    private fun checkConnections() {
        if (!SyncService.running) {
            Toast.makeText(this, "Turn syncing on first.", Toast.LENGTH_SHORT).show()
            return
        }
        checkButton.isEnabled = false
        checkButton.alpha = 0.38f
        Toast.makeText(this, "Checking connections…", Toast.LENGTH_SHORT).show()
        clipApp.background({ clipApp.node.checkDevices() }) { result ->
            checkButton.isEnabled = true
            checkButton.alpha = 1f
            result.onSuccess { list ->
                checks = list.associateBy { it.id }
                if (list.isEmpty()) Toast.makeText(this, "No other devices to check yet.", Toast.LENGTH_SHORT).show()
            }.onFailure {
                Toast.makeText(this, "Could not check: ${it.message}", Toast.LENGTH_LONG).show()
            }
            load()
        }
    }

    // --- sending automatically ------------------------------------------------

    private fun refreshAutoSend() {
        val enabled = AutoSend.enabled(this)
        val overlay = AutoSend.canOverlay(this)
        val log = AutoSend.canReadLog(this)
        val status = when {
            !enabled -> "Off. After you copy, tap Send clipboard or the button in the notification."
            overlay && log && SyncService.running -> "On. Whatever you copy on this phone goes to your devices."
            overlay && log -> "Ready. Turn syncing on to start."
            else -> "Finish the steps below to start."
        }
        val switch = M3Switch(this, look).apply {
            contentDescription = "Send automatically"
            setChecked(enabled)
            onChange = { on -> if (on) askAutoSend() else setAutoSend(false) }
        }
        sending.removeAllViews()
        look.addRow(sending, look.row("Send automatically", status, trailing = switch))
        if (!enabled) return
        if (!overlay) {
            look.addRow(
                sending,
                look.row(
                    "Allow display over other apps",
                    "Lets ClipCircle read the clipboard for a moment after each copy.",
                    trailing = look.button("Allow", Look.Kind.Tonal) {
                        startActivity(
                            Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:$packageName")),
                        )
                    },
                ),
            )
        }
        if (!log) look.addRow(sending, logStep())
    }

    private fun logStep(): View {
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(look.dp(16), look.dp(12), look.dp(16), look.dp(8))
        }
        column.addView(look.text("Allow reading the system log"))
        column.addView(
            look.text(
                "Connect this phone to a computer with USB debugging on and run this once. " +
                    "On Android 13 and later, choose Allow when asked about device logs.",
                14f,
                p.onSurfaceVariant,
            ),
        )
        val command = AutoSend.grantCommand(this)
        column.addView(
            look.text(command, 13f).apply {
                typeface = Typeface.MONOSPACE
                setTextIsSelectable(true)
                background = look.rounded(p.containerHighest, 12f)
                setPadding(look.dp(12), look.dp(10), look.dp(12), look.dp(10))
            },
            LinearLayout.LayoutParams(-1, -2).apply { topMargin = look.dp(10) },
        )
        column.addView(
            look.button("Copy command", Look.Kind.Text) {
                getSystemService(ClipboardManager::class.java)
                    .setPrimaryClip(ClipData.newPlainText("adb command", command))
            },
            LinearLayout.LayoutParams(-2, look.dp(40)).apply {
                gravity = Gravity.END
                topMargin = look.dp(4)
            },
        )
        return column
    }

    /** Says what Send automatically needs before turning it on. */
    private fun askAutoSend() {
        val (dialog, column) = look.sheet("Send copies automatically?")
        var chosen = false
        column.addView(
            body(
                "Android doesn't let apps in the background read the clipboard. To send each copy " +
                    "without a tap, ClipCircle needs two things:\n\n" +
                    "• Permission to read the system log, granted once from a computer. The log can hold " +
                    "details from other apps. ClipCircle only looks for the line saying the clipboard " +
                    "changed, and never saves or sends the log.\n\n" +
                    "• Permission to show over other apps. After each copy ClipCircle takes focus for a " +
                    "moment to read the clipboard, which can close an open keyboard.\n\n" +
                    "You can turn this off any time.",
            ),
        )
        column.addView(
            actions(
                look.button("Not now", Look.Kind.Text) { dialog.dismiss() },
                look.button("Turn on", Look.Kind.Filled) {
                    chosen = true
                    setAutoSend(true)
                    dialog.dismiss()
                },
            ),
        )
        dialog.setOnDismissListener { if (!chosen) refreshAutoSend() }
        dialog.show()
    }

    private fun setAutoSend(on: Boolean) {
        AutoSend.setEnabled(this, on)
        refreshAutoSend()
        if (SyncService.running) SyncService.start(this)
    }

    // --- pairing --------------------------------------------------------------

    private fun confirmRemove(device: DeviceRow) {
        val (dialog, column) = look.sheet("Remove ${device.name}?")
        column.addView(body("It stops sharing clips with this circle. To add it back, pair the two again."))
        column.addView(
            actions(
                look.button("Cancel", Look.Kind.Text) { dialog.dismiss() },
                look.button("Remove", Look.Kind.Filled) {
                    dialog.dismiss()
                    clipApp.background({ clipApp.node.removeDevice(device.id) }) { result ->
                        result.onFailure {
                            Toast.makeText(this, "Could not remove: ${it.message}", Toast.LENGTH_LONG).show()
                        }
                        load()
                    }
                },
            ),
        )
        dialog.show()
    }

    /** Shows a pairing code until a device joins with it. */
    private inner class PairSheet {
        val dialog: Dialog
        private val digits = LinearLayout(this@MainActivity).apply { gravity = Gravity.CENTER }
        private val progress = ProgressBar(this@MainActivity, null, android.R.attr.progressBarStyleHorizontal).apply {
            isIndeterminate = true
            indeterminateTintList = ColorStateList.valueOf(p.primary)
        }
        private val status = look.text("Starting…", 14f, p.onSurfaceVariant).apply { gravity = Gravity.CENTER }
        private val action: Button

        init {
            val (sheet, column) = look.sheet("Add a device")
            dialog = sheet
            val intro = SpannableStringBuilder("On the other device, choose ")
            val start = intro.length
            intro.append("Join a circle")
            intro.setSpan(StyleSpan(Typeface.BOLD), start, intro.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            intro.setSpan(ForegroundColorSpan(p.onSurface), start, intro.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            intro.append(" and type this code.")
            column.addView(body(intro))
            column.addView(digits, LinearLayout.LayoutParams(-1, look.dp(60)).apply { topMargin = look.dp(24) })
            column.addView(progress, LinearLayout.LayoutParams(-1, -2).apply { topMargin = look.dp(16) })
            column.addView(status, LinearLayout.LayoutParams(-1, -2))
            action = look.button("Cancel", Look.Kind.Text) { sheet.dismiss() }
            column.addView(actions(action))
            sheet.setOnDismissListener { if (pairSheet === this) pairSheet = null }
        }

        fun showCode(code: String) {
            digits.removeAllViews()
            code.forEachIndexed { index, digit ->
                digits.addView(
                    look.text(digit.toString(), 32f).apply {
                        gravity = Gravity.CENTER
                        fontFeatureSettings = "tnum"
                        background = look.rounded(p.containerHighest, 12f)
                    },
                    LinearLayout.LayoutParams(look.dp(44), look.dp(60)).apply {
                        marginStart = if (index == 3) look.dp(16) else look.dp(4)
                        marginEnd = look.dp(4)
                    },
                )
            }
            digits.contentDescription = "Pairing code ${code.toList().joinToString(" ")}"
            status.text = "Waiting for the other device…"
        }

        fun finished(ok: Boolean, message: String) {
            progress.visibility = View.INVISIBLE
            status.setTextColor(if (ok) p.onSurface else p.error)
            status.text = if (ok) "$message joined your circle." else "Pairing failed: $message"
            action.text = "Done"
        }
    }

    private fun startPairing() {
        val sheet = PairSheet()
        pairSheet = sheet
        sheet.dialog.show()
        if (!SyncService.running) {
            sheet.finished(false, "syncing is off. Turn it on, then try again.")
            return
        }
        clipApp.background({ clipApp.node.startPairing() }) { result ->
            result.onSuccess { sheet.showCode(it) }
                .onFailure { sheet.finished(false, it.message ?: "could not show a code") }
        }
    }

    private fun showJoin() {
        val (dialog, column) = look.sheet("Join a circle")
        column.addView(
            body("Type the 6-digit code shown on a device that's already in the circle. Both need to be on the same Wi-Fi."),
        )
        val field = EditText(this).apply {
            hint = "123456"
            inputType = InputType.TYPE_CLASS_NUMBER
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 28f)
            letterSpacing = 0.3f
            fontFeatureSettings = "tnum"
            setTextColor(p.onSurface)
            setHintTextColor(look.withAlpha(p.onSurfaceVariant, 0.5f))
            contentDescription = "Pairing code"
            background = StateListDrawable().apply {
                addState(
                    intArrayOf(android.R.attr.state_focused),
                    look.rounded(p.containerLow, 4f).apply { setStroke(look.dp(2), p.primary) },
                )
                addState(intArrayOf(), look.rounded(p.containerLow, 4f, p.outline))
            }
            setPadding(look.dp(16), 0, look.dp(16), 0)
        }
        column.addView(
            look.text("Pairing code", 12f, p.primary),
            LinearLayout.LayoutParams(-2, -2).apply {
                topMargin = look.dp(24)
                bottomMargin = look.dp(6)
            },
        )
        column.addView(field, LinearLayout.LayoutParams(-1, look.dp(64)))
        val helper = look.text("0 of 6 digits", 12f, p.onSurfaceVariant).apply {
            setPadding(look.dp(16), look.dp(6), look.dp(16), 0)
        }
        column.addView(helper)
        lateinit var join: Button
        fun enableJoin(on: Boolean) {
            join.isEnabled = on
            join.alpha = if (on) 1f else 0.38f
        }
        join = look.button("Join", Look.Kind.Filled) {
            enableJoin(false)
            field.isEnabled = false
            helper.setTextColor(p.onSurfaceVariant)
            helper.text = "Looking for the device…"
            val code = field.text.toString()
            clipApp.background({ clipApp.node.join(code) }) { result ->
                result.onSuccess { n ->
                    dialog.dismiss()
                    val others = if (n == 1u) "1 other device" else "$n other devices"
                    Toast.makeText(this, "Joined. $others in your circle.", Toast.LENGTH_LONG).show()
                }.onFailure {
                    field.isEnabled = true
                    enableJoin(true)
                    helper.setTextColor(p.error)
                    helper.text = "Could not join: ${it.message}"
                }
                load()
            }
        }
        enableJoin(false)
        field.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable) {
                // Codes are shown as "123 456"; accept them typed or pasted either way.
                val clean = s.filter { it.isDigit() }.take(6).toString()
                if (clean != s.toString()) {
                    field.setText(clean)
                    field.setSelection(clean.length)
                    return
                }
                helper.setTextColor(p.onSurfaceVariant)
                helper.text = "${clean.length} of 6 digits"
                enableJoin(clean.length == 6)
            }
        })
        column.addView(actions(look.button("Cancel", Look.Kind.Text) { dialog.dismiss() }, join))
        dialog.window?.setSoftInputMode(
            WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE or
                WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE,
        )
        field.requestFocus()
        dialog.show()
    }

    // --- layout ---------------------------------------------------------------

    private fun body(value: CharSequence) = look.text(value, 14f, p.onSurfaceVariant).apply {
        setLineSpacing(0f, 1.15f)
        setPadding(0, look.dp(8), 0, 0)
    }

    /** Sheet buttons, at the bottom right. */
    private fun actions(vararg buttons: Button) = LinearLayout(this).apply {
        gravity = Gravity.END
        setPadding(0, look.dp(24), 0, 0)
        buttons.forEachIndexed { index, button ->
            addView(
                button,
                LinearLayout.LayoutParams(-2, look.dp(40)).apply { if (index > 0) marginStart = look.dp(8) },
            )
        }
    }

    private fun buildUi(): View {
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            // Room to scroll the last row out from under the Send button.
            setPadding(0, 0, 0, look.dp(104))
        }

        val appBar = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(look.dp(16), 0, look.dp(4), 0)
        }
        appBar.addView(look.text("ClipCircle", 22f), LinearLayout.LayoutParams(0, -2, 1f))
        checkButton = look.iconButton(R.drawable.ic_refresh, "Check connections") { checkConnections() }
        appBar.addView(checkButton, LinearLayout.LayoutParams(look.dp(48), look.dp(48)))
        column.addView(appBar, LinearLayout.LayoutParams(-1, look.dp(64)))

        val card = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            background = look.rounded(p.container, 28f)
            setPadding(look.dp(20), look.dp(12), look.dp(16), look.dp(12))
        }
        val texts = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        nameView = look.text("", bold = true)
        syncLine = look.text("Starting…", 14f, p.onSurfaceVariant)
        texts.addView(nameView)
        texts.addView(syncLine)
        card.addView(texts, LinearLayout.LayoutParams(0, -2, 1f).apply { marginEnd = look.dp(16) })
        syncSwitch = M3Switch(this, look).apply {
            contentDescription = "Sync"
            onChange = { on ->
                if (on) SyncService.start(this@MainActivity) else SyncService.stop(this@MainActivity)
                ticker.postDelayed({ load() }, 500)
            }
        }
        card.addView(syncSwitch)
        column.addView(card, margins(LinearLayout.LayoutParams(-1, -2)))

        val buttons = LinearLayout(this)
        buttons.addView(
            look.button("Add a device", Look.Kind.Tonal, R.drawable.ic_add) { startPairing() },
            LinearLayout.LayoutParams(0, look.dp(40), 1f).apply { marginEnd = look.dp(8) },
        )
        buttons.addView(
            look.button("Join a circle", Look.Kind.Outlined) { showJoin() },
            LinearLayout.LayoutParams(0, look.dp(40), 1f),
        )
        column.addView(buttons, margins(LinearLayout.LayoutParams(-1, -2)).apply { topMargin = look.dp(16) })

        devicesHeading = look.heading("Devices")
        column.addView(devicesHeading)
        devices = look.group()
        column.addView(devices, margins(LinearLayout.LayoutParams(-1, -2)))

        column.addView(look.heading("Sending"))
        sending = look.group()
        column.addView(sending, margins(LinearLayout.LayoutParams(-1, -2)))

        column.addView(look.heading("Recent"))
        recent = look.group()
        column.addView(recent, margins(LinearLayout.LayoutParams(-1, -2)))

        val fab = Button(this).apply {
            text = "Send clipboard"
            isAllCaps = false
            typeface = look.medium
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            setTextColor(p.onPrimaryContainer)
            stateListAnimator = null
            minWidth = 0
            minimumWidth = 0
            background = look.ripple(look.rounded(p.primaryContainer, 16f), 16f, p.onPrimaryContainer)
            elevation = look.dpf(6f)
            setPadding(look.dp(16), 0, look.dp(20), 0)
            setCompoundDrawablesRelative(look.icon(R.drawable.ic_send, p.onPrimaryContainer), null, null, null)
            compoundDrawablePadding = look.dp(12)
            setOnClickListener { startActivity(Intent(this@MainActivity, SendActivity::class.java)) }
        }

        return FrameLayout(this).apply {
            setBackgroundColor(p.surface)
            addView(ScrollView(this@MainActivity).apply { addView(column) })
            addView(
                fab,
                FrameLayout.LayoutParams(-2, look.dp(56), Gravity.BOTTOM or Gravity.END).apply {
                    setMargins(look.dp(16), look.dp(16), look.dp(16), look.dp(24))
                },
            )
            // Android 15 draws apps under the status and navigation bars.
            if (Build.VERSION.SDK_INT >= 30) {
                setOnApplyWindowInsetsListener { view, insets ->
                    val bars = insets.getInsets(WindowInsets.Type.systemBars())
                    view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                    insets
                }
            }
        }
    }

    private fun margins(params: LinearLayout.LayoutParams) = params.apply {
        marginStart = look.dp(16)
        marginEnd = look.dp(16)
    }
}
