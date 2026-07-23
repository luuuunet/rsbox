import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';

/// Cloudflare Turnstile — 在应用内完成验证并回传 token。
class TurnstileCaptcha extends StatefulWidget {
  const TurnstileCaptcha({
    super.key,
    required this.siteKey,
    this.onToken,
    this.height = 72,
  });

  final String siteKey;
  final ValueChanged<String>? onToken;
  final double height;

  @override
  State<TurnstileCaptcha> createState() => _TurnstileCaptchaState();
}

class _TurnstileCaptchaState extends State<TurnstileCaptcha> {
  late final WebViewController _controller;
  bool _ready = false;

  @override
  void initState() {
    super.initState();
    _controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setBackgroundColor(Colors.transparent)
      ..addJavaScriptChannel(
        'TurnstileBridge',
        onMessageReceived: (message) {
          final token = message.message.trim();
          if (token.isNotEmpty) {
            widget.onToken?.call(token);
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
      ..loadHtmlString(_htmlForSiteKey(widget.siteKey));
  }

  @override
  void didUpdateWidget(covariant TurnstileCaptcha oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.siteKey != widget.siteKey) {
      setState(() => _ready = false);
      _controller.loadHtmlString(_htmlForSiteKey(widget.siteKey));
    }
  }

  Future<void> reset() async {
    widget.onToken?.call('');
    await _controller.reload();
  }

  @override
  Widget build(BuildContext context) {
    return SizedBox(
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
    );
  }

  String _htmlForSiteKey(String siteKey) {
    final encodedKey = jsonEncode(siteKey);
    return '''
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <script src="https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit" async defer></script>
  <style>
    html, body { margin: 0; padding: 0; background: transparent; }
    #turnstile { display: flex; justify-content: center; align-items: center; min-height: 65px; }
  </style>
</head>
<body>
  <div id="turnstile"></div>
  <script>
    function renderTurnstile() {
      if (typeof turnstile === 'undefined') {
        setTimeout(renderTurnstile, 120);
        return;
      }
      turnstile.render('#turnstile', {
        sitekey: $encodedKey,
        theme: 'dark',
        callback: function(token) { TurnstileBridge.postMessage(token); },
        'expired-callback': function() { TurnstileBridge.postMessage(''); },
        'error-callback': function() { TurnstileBridge.postMessage(''); }
      });
    }
    renderTurnstile();
  </script>
</body>
</html>
''';
  }
}
