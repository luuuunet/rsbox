import 'dart:io';

import 'package:singbox_mm/singbox_mm.dart';

/// Shared sing-box runtime for Android (libbox + VpnService).
class SingboxRuntime {
  SingboxRuntime._();

  static final SingboxRuntime instance = SingboxRuntime._();

  final SignboxVpn vpn = SignboxVpn();
  Future<void>? _initFuture;

  Future<void> ensureInitialized() {
    if (!Platform.isAndroid) {
      return Future<void>.value();
    }
    return _initFuture ??= _initialize();
  }

  Future<void> _initialize() async {
    await vpn.initialize(
      const SingboxRuntimeOptions(
        logLevel: 'warn',
        tunInterfaceName: 'g5-tun',
        tunInet4Address: '172.19.0.1/30',
      ),
    );
  }

  Future<bool> ensureVpnPermission() async {
    await ensureInitialized();
    return vpn.requestVpnPermission();
  }

  Future<bool> ensureNotificationPermission() async {
    await ensureInitialized();
    return vpn.requestNotificationPermission();
  }

  Future<void> dispose() async {
    await vpn.dispose();
    _initFuture = null;
  }
}
