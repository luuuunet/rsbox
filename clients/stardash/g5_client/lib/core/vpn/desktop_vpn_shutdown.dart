import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../providers/vpn_providers.dart';
import 'linux_port_helper.dart';
import 'linux_system_proxy.dart';
import 'windows_port_helper.dart';
import 'windows_system_proxy.dart';

/// 桌面端退出时停止 sing-box 并恢复系统代理。
abstract final class DesktopVpnShutdown {
  static bool _running = false;

  static bool get isDesktop =>
      Platform.isWindows || Platform.isLinux;

  static Future<void> shutdown(WidgetRef ref) async {
    if (!isDesktop || _running) return;
    _running = true;
    try {
      await ref.read(vpnControllerProvider).stop();
    } catch (_) {}
    await _killAndRestoreProxy();
  }

  static Future<void> shutdownWithoutRef() async {
    if (!isDesktop || _running) return;
    _running = true;
    await _killAndRestoreProxy();
  }

  static Future<void> _killAndRestoreProxy() async {
    if (Platform.isWindows) {
      try {
        await WindowsSystemProxy.disable();
      } catch (_) {}
      await WindowsPortHelper.killOrphanSingBox();
    } else if (Platform.isLinux) {
      try {
        await LinuxSystemProxy.disable();
      } catch (_) {}
      await LinuxPortHelper.killOrphanSingBox();
    }
  }
}
