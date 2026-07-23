package com.g5panel.g5_client

import android.content.pm.PackageManager
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    private val channelName = "com.g5panel.g5_client/device"

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, channelName)
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "isTelevision" -> result.success(isTelevision())
                    "libboxNativeAvailable" -> result.success(libboxNativeAvailable())
                    else -> result.notImplemented()
                }
            }
    }

    private fun libboxNativeAvailable(): Boolean {
        val extracted = java.io.File(applicationInfo.nativeLibraryDir, "libbox.so")
        if (extracted.exists()) return true

        // extractNativeLibs=false loads .so directly from the APK (no extracted file).
        val abi = android.os.Build.SUPPORTED_ABIS.firstOrNull() ?: return false
        return try {
            java.util.zip.ZipFile(applicationInfo.sourceDir).use { zip ->
                zip.getEntry("lib/$abi/libbox.so") != null
            }
        } catch (_: Exception) {
            false
        }
    }

    private fun isTelevision(): Boolean {
        val uiMode = resources.configuration.uiMode and
            android.content.res.Configuration.UI_MODE_TYPE_MASK
        if (uiMode == android.content.res.Configuration.UI_MODE_TYPE_TELEVISION) {
            return true
        }
        return packageManager.hasSystemFeature(PackageManager.FEATURE_LEANBACK)
    }
}
