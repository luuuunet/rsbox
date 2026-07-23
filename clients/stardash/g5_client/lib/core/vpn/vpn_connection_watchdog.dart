import 'dart:async';

import 'vpn_health_monitor.dart';

/// 主动探测隧道健康；连续失败后触发 [onReconnect]。
class VpnConnectionWatchdog {
  VpnConnectionWatchdog({
    required this.isEnabled,
    required this.isConnected,
    required this.proxyPort,
    required this.onReconnect,
  });

  final bool Function() isEnabled;
  final bool Function() isConnected;
  final int? Function() proxyPort;
  final Future<void> Function() onReconnect;

  static const probeInterval = Duration(seconds: 45);
  static const consecutiveFailuresThreshold = 2;

  Timer? _timer;
  int _consecutiveFailures = 0;
  bool _reconnecting = false;

  void bindConnected(bool connected) {
    if (connected && isEnabled()) {
      start();
    } else {
      stop();
    }
  }

  void bindEnabled(bool enabled) {
    if (enabled && isConnected()) {
      start();
    } else {
      stop();
    }
  }

  void start() {
    if (_timer != null) return;
    _consecutiveFailures = 0;
    _timer = Timer.periodic(probeInterval, (_) {
      unawaited(_probe());
    });
  }

  void stop() {
    _timer?.cancel();
    _timer = null;
    _consecutiveFailures = 0;
  }

  void dispose() => stop();

  Future<void> _probe() async {
    if (!isEnabled() || !isConnected() || _reconnecting) return;

    final ok = await VpnHealthMonitor.checkConnectivity(
      proxyPort: proxyPort(),
    );

    if (!isEnabled() || !isConnected()) return;

    if (ok) {
      _consecutiveFailures = 0;
      return;
    }

    _consecutiveFailures++;
    if (_consecutiveFailures < consecutiveFailuresThreshold) return;

    _consecutiveFailures = 0;
    _reconnecting = true;
    try {
      await onReconnect();
    } finally {
      _reconnecting = false;
    }
  }
}
