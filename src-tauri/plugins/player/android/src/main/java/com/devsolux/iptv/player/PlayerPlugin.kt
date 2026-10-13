package com.devsolux.iptv.player

import android.app.Activity
import android.app.Application
import android.app.Dialog
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import android.view.KeyEvent
import android.view.ViewGroup
import android.view.Window
import android.view.WindowManager
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.ui.PlayerView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.IOException

@InvokeArg
class PlaybackArgs {
    lateinit var sessionId: String
    lateinit var url: String
    lateinit var title: String
    lateinit var kind: String
}
@InvokeArg
class StopArgs { lateinit var sessionId: String }

@UnstableApi
@TauriPlugin
class PlayerPlugin(private val activity: Activity) : Plugin(activity) {
    private var player: ExoPlayer? = null
    private var dialog: Dialog? = null
    private var currentId: String? = null
    private var pending: Invoke? = null

    private val lifecycle = object : Application.ActivityLifecycleCallbacks {
        override fun onActivityStopped(a: Activity) { if (a === activity) finishPlayback() }
        override fun onActivityDestroyed(a: Activity) {
            if (a === activity) {
                finishPlayback()
                activity.application.unregisterActivityLifecycleCallbacks(this)
            }
        }
        override fun onActivityCreated(a: Activity, state: Bundle?) = Unit
        override fun onActivityStarted(a: Activity) = Unit
        override fun onActivityResumed(a: Activity) = Unit
        override fun onActivityPaused(a: Activity) = Unit
        override fun onActivitySaveInstanceState(a: Activity, out: Bundle) = Unit
    }

    init { activity.application.registerActivityLifecycleCallbacks(lifecycle) }

    override fun load(webView: WebView) {
        val owner = activity as? ComponentActivity ?: return
        owner.onBackPressedDispatcher.addCallback(owner, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (dialog != null) finishPlayback()
                else webView.evaluateJavascript("window.dispatchEvent(new CustomEvent('iptv-native-back'));", null)
            }
        })
    }

    @Command
    fun play(invoke: Invoke) { activity.runOnUiThread { playOnMain(invoke) } }

    private fun playOnMain(invoke: Invoke) {
        val args = invoke.parseArgs(PlaybackArgs::class.java)
        val uri = Uri.parse(args.url)
        if (uri.scheme != "http" || uri.host != "127.0.0.1" ||
            uri.port < 1 || !uri.path.orEmpty().startsWith("/media/${args.sessionId}/")) {
            invoke.reject("Invalid playback session")
            return
        }
        finishPlayback()
        currentId = args.sessionId
        pending = invoke
        try {
            val http = DefaultHttpDataSource.Factory()
                .setConnectTimeoutMs(10000).setReadTimeoutMs(30000)
                .setAllowCrossProtocolRedirects(false)
            // The native engine may only consume the authorized Rust relay session.
            // Provider headers, host policy and redirects have one owner: Rust.
            val transport = ResolvingDataSource.Factory(http) { spec ->
                if (spec.uri.scheme != "http" || spec.uri.host != "127.0.0.1" ||
                    spec.uri.port != uri.port ||
                    !spec.uri.path.orEmpty().startsWith("/media/${args.sessionId}/")) {
                    throw IOException("Media request is outside the active session")
                }
                spec
            }
            val load = DefaultLoadControl.Builder()
                .setBufferDurationsMs(3000, 15000, 500, 1000)
                .setTargetBufferBytes(12 * 1024 * 1024)
                .setPrioritizeTimeOverSizeThresholds(false)
                .build()
            val exo = ExoPlayer.Builder(activity)
                .setRenderersFactory(DefaultRenderersFactory(activity).setEnableDecoderFallback(true))
                .setMediaSourceFactory(DefaultMediaSourceFactory(transport))
                .setLoadControl(load).build()
            player = exo
            exo.setAudioAttributes(AudioAttributes.Builder()
                .setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MOVIE).build(), true)
            exo.setHandleAudioBecomingNoisy(true)
            val view = PlayerView(activity).apply {
                player = exo
                useController = true
                controllerShowTimeoutMs = 5000
                keepScreenOn = true
                isFocusable = true
            }
            val frame = FrameLayout(activity).apply {
                setBackgroundColor(Color.BLACK)
                addView(view, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            }
            var retries = 0
            exo.addListener(object : Player.Listener {
                override fun onPlayerError(error: PlaybackException) {
                    // Never render provider URLs or native exception messages.
                    val panel = LinearLayout(activity).apply {
                        orientation = LinearLayout.VERTICAL
                        setPadding(48, 48, 48, 48)
                        setBackgroundColor(Color.BLACK)
                    }
                    panel.addView(TextView(activity).apply {
                        text = "Playback stopped: ${error.errorCodeName}"
                        setTextColor(Color.WHITE)
                        textSize = 20f
                    })
                    val retry = Button(activity).apply {
                        text = if (retries < 2) "Retry" else "Close player"
                        setOnClickListener {
                            if (retries >= 2) finishPlayback()
                            else { retries++; frame.removeView(panel); exo.prepare(); exo.play(); view.requestFocus() }
                        }
                    }
                    panel.addView(retry)
                    frame.addView(panel)
                    retry.requestFocus()
                }
            })
            val window = Dialog(activity, android.R.style.Theme_Material_NoActionBar_Fullscreen).apply {
                requestWindowFeature(Window.FEATURE_NO_TITLE)
                setContentView(frame)
                setOnDismissListener { finishPlayback() }
                setOnKeyListener { _, key, event ->
                    if (event.action != KeyEvent.ACTION_DOWN) false
                    else when (key) {
                        KeyEvent.KEYCODE_BACK, KeyEvent.KEYCODE_ESCAPE -> { dismiss(); true }
                        KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE -> { if (exo.isPlaying) exo.pause() else exo.play(); true }
                        KeyEvent.KEYCODE_MEDIA_PLAY -> { exo.play(); true }
                        KeyEvent.KEYCODE_MEDIA_PAUSE -> { exo.pause(); true }
                        KeyEvent.KEYCODE_MENU -> { view.showController(); true }
                        else -> false
                    }
                }
            }
            dialog = window
            window.show()
            window.window?.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            window.window?.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
            val media = MediaItem.Builder().setUri(uri)
            when (args.kind) {
                "hls" -> media.setMimeType(MimeTypes.APPLICATION_M3U8)
                "dash" -> media.setMimeType(MimeTypes.APPLICATION_MPD)
            }
            exo.setMediaItem(media.build())
            exo.prepare()
            exo.playWhenReady = true
            view.requestFocus()
        } catch (_: Exception) {
            val call = pending
            pending = null
            finishPlayback()
            call?.reject("Native player initialization failed")
        }
    }

    @Command
    fun stop(invoke: Invoke) { activity.runOnUiThread { stopOnMain(invoke) } }

    private fun stopOnMain(invoke: Invoke) {
        val args = invoke.parseArgs(StopArgs::class.java)
        if (currentId == args.sessionId) finishPlayback()
        invoke.resolve(JSObject())
    }

    private fun finishPlayback() {
        val call = pending
        pending = null
        currentId = null
        val old = dialog
        dialog = null
        old?.setOnDismissListener(null)
        old?.dismiss()
        player?.release()
        player = null
        call?.resolve(JSObject())
    }
}
