package dev.farhanlabib.clipcircle

import android.animation.ValueAnimator
import android.app.Dialog
import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.os.Build
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.Button
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.Switch
import android.widget.TextView
import android.view.accessibility.AccessibilityNodeInfo

/** Material 3 colors for ClipCircle's teal, light or dark to match the system. */
class Palette(
    val primary: Int,
    val onPrimary: Int,
    val primaryContainer: Int,
    val onPrimaryContainer: Int,
    val secondaryContainer: Int,
    val onSecondaryContainer: Int,
    val surface: Int,
    val containerLow: Int,
    val container: Int,
    val containerHighest: Int,
    val onSurface: Int,
    val onSurfaceVariant: Int,
    val outline: Int,
    val outlineVariant: Int,
    val error: Int,
) {
    companion object {
        fun of(context: Context): Palette {
            val night = context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK ==
                Configuration.UI_MODE_NIGHT_YES
            return if (night) dark else light
        }

        private fun c(hex: String) = Color.parseColor(hex)

        private val light = Palette(
            primary = c("#006A63"), onPrimary = c("#FFFFFF"),
            primaryContainer = c("#9FF2E8"), onPrimaryContainer = c("#00201D"),
            secondaryContainer = c("#CCE8E3"), onSecondaryContainer = c("#051F1C"),
            surface = c("#F4FBF9"), containerLow = c("#EEF5F3"), container = c("#E9EFED"),
            containerHighest = c("#E3EAE7"),
            onSurface = c("#161D1C"), onSurfaceVariant = c("#3F4947"),
            outline = c("#6F7977"), outlineVariant = c("#BEC9C6"), error = c("#BA1A1A"),
        )

        private val dark = Palette(
            primary = c("#82D5CC"), onPrimary = c("#003733"),
            primaryContainer = c("#00504B"), onPrimaryContainer = c("#9FF2E8"),
            secondaryContainer = c("#324B48"), onSecondaryContainer = c("#CCE8E3"),
            surface = c("#0E1513"), containerLow = c("#161D1C"), container = c("#1A2120"),
            containerHighest = c("#252B2A"),
            onSurface = c("#DDE4E1"), onSurfaceVariant = c("#BEC9C6"),
            outline = c("#899390"), outlineVariant = c("#3F4947"), error = c("#FFB4AB"),
        )
    }
}

/** Builds the app's views in code, in Material 3 style. */
class Look(val context: Context) {
    val p = Palette.of(context)
    val medium: Typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)

    fun dp(value: Int) = (value * context.resources.displayMetrics.density).toInt()
    fun dpf(value: Float) = value * context.resources.displayMetrics.density

    fun text(value: CharSequence, sp: Float = 16f, color: Int = p.onSurface, bold: Boolean = false) =
        TextView(context).apply {
            text = value
            setTextSize(TypedValue.COMPLEX_UNIT_SP, sp)
            setTextColor(color)
            if (bold) typeface = medium
        }

    fun rounded(color: Int, radius: Float, stroke: Int? = null) = GradientDrawable().apply {
        setColor(color)
        cornerRadius = dpf(radius)
        if (stroke != null) setStroke(dp(1), stroke)
    }

    /** [content] with a ripple clipped to the same rounded shape. */
    fun ripple(content: Drawable?, radius: Float, rippleOn: Int = p.onSurface) = RippleDrawable(
        ColorStateList.valueOf(withAlpha(rippleOn, 0.12f)),
        content,
        rounded(Color.WHITE, radius),
    )

    fun withAlpha(color: Int, alpha: Float) = Color.argb((alpha * 255).toInt(), Color.red(color), Color.green(color), Color.blue(color))

    fun icon(res: Int, tint: Int, size: Int = 24): Drawable =
        context.getDrawable(res)!!.mutate().apply {
            setTint(tint)
            setBounds(0, 0, dp(size), dp(size))
        }

    enum class Kind { Filled, Tonal, Outlined, Text }

    fun button(label: String, kind: Kind, iconRes: Int? = null, onClick: () -> Unit) = Button(context).apply {
        text = label
        isAllCaps = false
        typeface = medium
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
        letterSpacing = 0.01f
        stateListAnimator = null
        minWidth = 0
        minimumWidth = 0
        minHeight = 0
        minimumHeight = dp(40)
        gravity = Gravity.CENTER
        val (bg, fg) = when (kind) {
            Kind.Filled -> p.primary to p.onPrimary
            Kind.Tonal -> p.secondaryContainer to p.onSecondaryContainer
            Kind.Outlined -> Color.TRANSPARENT to p.primary
            Kind.Text -> Color.TRANSPARENT to p.primary
        }
        setTextColor(fg)
        val stroke = if (kind == Kind.Outlined) p.outline else null
        background = ripple(rounded(bg, 20f, stroke), 20f, fg)
        val side = if (kind == Kind.Text) dp(12) else dp(24)
        setPadding(if (iconRes != null) dp(16) else side, 0, side, 0)
        if (iconRes != null) {
            setCompoundDrawablesRelative(icon(iconRes, fg, 18), null, null, null)
            compoundDrawablePadding = dp(8)
        }
        setOnClickListener { onClick() }
    }

    fun iconButton(res: Int, description: String, onClick: (View) -> Unit) = ImageButton(context).apply {
        setImageDrawable(icon(res, p.onSurfaceVariant))
        contentDescription = description
        background = ripple(null, 24f)
        scaleType = ImageView.ScaleType.CENTER
        layoutParams = LinearLayout.LayoutParams(dp(48), dp(48))
        setOnClickListener(onClick)
    }

    /** Small primary-colored heading over a list. */
    fun heading(value: String) = text(value, 14f, p.primary, bold = true).apply {
        letterSpacing = 0.01f
        setPadding(dp(16), dp(20), dp(16), dp(8))
    }

    /** A rounded group of rows with thin lines between them. */
    fun group() = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        background = rounded(p.containerLow, 24f)
        clipToOutline = true
    }

    fun addRow(group: LinearLayout, row: View) {
        if (group.childCount > 0) {
            group.addView(View(context).apply { setBackgroundColor(p.outlineVariant) }, LinearLayout.LayoutParams(-1, 1))
        }
        group.addView(row, LinearLayout.LayoutParams(-1, -2))
    }

    /** A list row: optional leading view, a title and subtitle, optional trailing view. */
    fun row(
        title: CharSequence,
        subtitle: CharSequence?,
        leading: View? = null,
        trailing: View? = null,
        singleLine: Boolean = false,
        subtitleColor: Int = p.onSurfaceVariant,
    ): LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = dp(64)
        setPadding(dp(16), dp(8), if (trailing != null) dp(8) else dp(16), dp(8))
        if (leading != null) {
            addView(leading)
            (leading.layoutParams as LinearLayout.LayoutParams).marginEnd = dp(16)
        }
        val texts = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        texts.addView(text(title).apply {
            letterSpacing = 0.03f
            if (singleLine) {
                maxLines = 1
                ellipsize = android.text.TextUtils.TruncateAt.END
            }
        })
        if (subtitle != null) texts.addView(text(subtitle, 14f, subtitleColor))
        addView(texts, LinearLayout.LayoutParams(0, -2, 1f))
        if (trailing != null) addView(trailing)
    }

    /** A round tile with an icon, leading a device row. */
    fun avatar(res: Int, bright: Boolean) = ImageView(context).apply {
        val (bg, fg) = if (bright) p.primaryContainer to p.onPrimaryContainer else p.containerHighest to p.outline
        background = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(bg)
        }
        setImageDrawable(icon(res, fg, 20))
        scaleType = ImageView.ScaleType.CENTER
        layoutParams = LinearLayout.LayoutParams(dp(40), dp(40))
    }

    fun leadingIcon(res: Int) = ImageView(context).apply {
        setImageDrawable(icon(res, p.onSurfaceVariant))
        layoutParams = LinearLayout.LayoutParams(dp(24), dp(24))
    }

    /**
     * A sheet that slides up from the bottom, with a drag handle and a title.
     * Add content to the returned column.
     */
    fun sheet(title: String): Pair<Dialog, LinearLayout> {
        val dialog = Dialog(context)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        val column = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = GradientDrawable().apply {
                setColor(p.containerLow)
                val r = dpf(28f)
                cornerRadii = floatArrayOf(r, r, r, r, 0f, 0f, 0f, 0f)
            }
            setPadding(dp(24), 0, dp(24), dp(24))
        }
        column.addView(View(context).apply {
            background = rounded(withAlpha(p.outline, 0.6f), 2f)
        }, LinearLayout.LayoutParams(dp(32), dp(4)).apply {
            gravity = Gravity.CENTER_HORIZONTAL
            topMargin = dp(22)
            bottomMargin = dp(18)
        })
        column.addView(text(title, 24f))
        if (Build.VERSION.SDK_INT >= 30) {
            val bottom = column.paddingBottom
            column.setOnApplyWindowInsetsListener { view, insets ->
                val bars = insets.getInsets(WindowInsets.Type.navigationBars())
                view.setPadding(view.paddingLeft, view.paddingTop, view.paddingRight, bottom + bars.bottom)
                insets
            }
        }
        dialog.setContentView(column)
        dialog.window?.apply {
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            setGravity(Gravity.BOTTOM)
            setDimAmount(0.32f)
            setWindowAnimations(android.R.style.Animation_InputMethod)
            setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
        }
        return dialog to column
    }
}

/** A Material 3 switch: a 52×32 track with a thumb that grows when on. */
class M3Switch(context: Context, private val look: Look) : View(context) {
    private val p = look.p
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private var position = 0f
    private var animator: ValueAnimator? = null

    var isChecked = false
        private set

    /** Called when the user flips the switch. */
    var onChange: ((Boolean) -> Unit)? = null

    init {
        isClickable = true
        isFocusable = true
        setOnClickListener {
            setChecked(!isChecked, animate = true)
            onChange?.invoke(isChecked)
        }
    }

    fun setChecked(on: Boolean, animate: Boolean = false) {
        if (on == isChecked) return
        isChecked = on
        animator?.cancel()
        val target = if (on) 1f else 0f
        if (!animate) {
            position = target
            invalidate()
            return
        }
        animator = ValueAnimator.ofFloat(position, target).apply {
            duration = 150
            addUpdateListener {
                position = it.animatedValue as Float
                invalidate()
            }
            start()
        }
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        setMeasuredDimension(look.dp(52), look.dp(48))
    }

    override fun onDraw(canvas: Canvas) {
        val trackTop = (height - look.dpf(32f)) / 2
        val track = RectF(0f, trackTop, width.toFloat(), trackTop + look.dpf(32f))
        val r = look.dpf(16f)
        paint.style = Paint.Style.FILL
        paint.color = blend(p.containerHighest, p.primary, position)
        canvas.drawRoundRect(track, r, r, paint)
        if (position < 1f) {
            paint.style = Paint.Style.STROKE
            paint.strokeWidth = look.dpf(2f)
            paint.color = look.withAlpha(p.outline, 1f - position)
            val inset = look.dpf(1f)
            canvas.drawRoundRect(
                RectF(track.left + inset, track.top + inset, track.right - inset, track.bottom - inset),
                r - inset, r - inset, paint,
            )
        }
        paint.style = Paint.Style.FILL
        paint.color = blend(p.outline, p.onPrimary, position)
        val radius = look.dpf(8f + 4f * position)
        val cx = track.left + r + (track.width() - 2 * r) * position
        canvas.drawCircle(cx, track.centerY(), radius, paint)
    }

    override fun setEnabled(enabled: Boolean) {
        super.setEnabled(enabled)
        alpha = if (enabled) 1f else 0.38f
    }

    override fun getAccessibilityClassName(): CharSequence = Switch::class.java.name

    override fun onInitializeAccessibilityNodeInfo(info: AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info)
        info.isCheckable = true
        info.isChecked = isChecked
    }

    private fun blend(from: Int, to: Int, t: Float): Int = Color.argb(
        (Color.alpha(from) + (Color.alpha(to) - Color.alpha(from)) * t).toInt(),
        (Color.red(from) + (Color.red(to) - Color.red(from)) * t).toInt(),
        (Color.green(from) + (Color.green(to) - Color.green(from)) * t).toInt(),
        (Color.blue(from) + (Color.blue(to) - Color.blue(from)) * t).toInt(),
    )
}
