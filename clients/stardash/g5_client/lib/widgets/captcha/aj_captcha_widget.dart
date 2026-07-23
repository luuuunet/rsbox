import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';

import '../../config/app_config.dart';

/// AJ 滑动拼图验证码 — 加载面板静态资源并在应用内完成验证。
class AjCaptchaWidget extends StatefulWidget {
  const AjCaptchaWidget({
    super.key,
    this.onVerification,
    this.height = 240,
  });

  final ValueChanged<String>? onVerification;
  final double height;

  @override
  State<AjCaptchaWidget> createState() => _AjCaptchaWidgetState();
}

class _AjCaptchaWidgetState extends State<AjCaptchaWidget> {
  late final WebViewController _controller;
  bool _ready = false;

  @override
  void initState() {
    super.initState();
    _controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setBackgroundColor(const Color(0xFF111318))
      ..addJavaScriptChannel(
        'AjCaptchaBridge',
        onMessageReceived: (message) {
          final token = message.message.trim();
          if (token.isNotEmpty) {
            widget.onVerification?.call(token);
          }
        },
      )
      ..setNavigationDelegate(
        NavigationDelegate(
          onPageFinished: (_) {
            if (mounted) setState(() => _ready = true);
          },
        ),
      )
      ..loadHtmlString(_htmlForPanel(AppConfig.panelBaseUrl));
  }

  Future<void> reset() async {
    widget.onVerification?.call('');
    await _controller.reload();
  }

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(12),
      child: SizedBox(
        height: widget.height,
        child: Stack(
          children: [
            WebViewWidget(controller: _controller),
            if (!_ready)
              const Center(
                child: SizedBox(
                  width: 22,
                  height: 22,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
              ),
          ],
        ),
      ),
    );
  }

  String _htmlForPanel(String baseUrl) {
    final root = baseUrl.endsWith('/') ? baseUrl.substring(0, baseUrl.length - 1) : baseUrl;
    final encodedRoot = jsonEncode('$root/');
    return '''
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="$root/ajcaptcha/verify.css">
  <link rel="stylesheet" href="$root/css/ajcaptcha-auth.css">
  <style>
    html, body { margin: 0; padding: 8px; background: #111318; color: #e8eaed; font-family: sans-serif; }
    .hint { font-size: 13px; margin-bottom: 8px; opacity: 0.85; }
  </style>
</head>
<body>
  <div class="hint">请拖动滑块完成拼图验证</div>
  <div id="aj-captcha-panel" data-base-url=$encodedRoot></div>
  <input type="hidden" id="captchaVerification">
  <script src="$root/vendor/jquery/3.7.1/jquery.min.js"></script>
  <script src="$root/ajcaptcha/aes.js"></script>
  <script src="$root/ajcaptcha/verify.js"></script>
  <script src="$root/js/ajcaptcha-auth.js"></script>
  <script>
    var last = '';
    setInterval(function () {
      var input = document.getElementById('captchaVerification');
      if (!input) return;
      var value = input.value || '';
      if (value && value !== last) {
        last = value;
        AjCaptchaBridge.postMessage(value);
      }
    }, 400);
  </script>
</body>
</html>
''';
  }
}
