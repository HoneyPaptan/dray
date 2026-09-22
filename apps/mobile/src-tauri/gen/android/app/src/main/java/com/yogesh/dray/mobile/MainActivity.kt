package com.yogesh.dray.mobile

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var webView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    // Back is one signal for every "go up a level" the app has, and only the
    // frontend knows what is open — a settings sheet, the inspector, the
    // sessions drawer. TauriActivity turns WryActivity's own handling off
    // (`handleBackNavigation = false`), so with nothing registered here the
    // system finishes the activity: the reader swipes back out of a sheet and
    // the app quits instead of closing the sheet.
    //
    // So ask the frontend first. `window.__drayBack` answers `true` when it
    // closed something; anything else — `false`, `null` from an older bundle,
    // an error — means there was nothing left and the activity may go.
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        val view = webView
        if (view == null) {
          finish()
          return
        }

        // Asynchronous, and its callback lands on the UI thread, so the press
        // is answered a frame later than a native one would be. That is the
        // price of the frontend owning the answer.
        view.evaluateJavascript("window.__drayBack ? window.__drayBack() : false") { handled ->
          if (handled != "true") finish()
        }
      }
    })
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
  }
}
