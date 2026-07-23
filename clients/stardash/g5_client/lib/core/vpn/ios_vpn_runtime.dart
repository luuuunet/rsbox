import 'dart:async';
import 'dart:io';

import 'package:singbox_mm/singbox_mm.dart';

/// sing-box VPN runtime for iOS (NetworkExtension + Libbox).
class IosVpnRuntime {
  IosVpnRuntime._();

  static final IosVpnRuntime instance = IosVpnRuntime._();

  final SignboxVpn vpn = SignboxVpn();
  Future<void>? _initFuture;

  Future<void> ensureInitialized() {
    if (!Platform.isIOS) return Future<void>.value();
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

  Future<void> dispose() async {
    await vpn.dispose();
    _initFuture = null;
  }
}
