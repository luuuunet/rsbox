import 'dart:async';
import 'dart:ui' show AppExitResponse;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/vpn/desktop_vpn_shutdown.dart';
import '../providers/vpn_providers.dart';

/// 监听应用退出，确保 sing-box 子进程与系统代理被清理。
class AppLifecycleGuard extends ConsumerStatefulWidget {
  const AppLifecycleGuard({super.key, required this.child});

  final Widget child;

  @override
  ConsumerState<AppLifecycleGuard> createState() => _AppLifecycleGuardState();
}

class _AppLifecycleGuardState extends ConsumerState<AppLifecycleGuard>
    with WidgetsBindingObserver {
  AppLifecycleListener? _exitListener;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    if (DesktopVpnShutdown.isDesktop) {
      _exitListener = AppLifecycleListener(
        onExitRequested: () async {
          await DesktopVpnShutdown.shutdown(ref);
          return AppExitResponse.exit;
        },
      );
    }
  }

  @override
  void dispose() {
    _exitListener?.dispose();
    WidgetsBinding.instance.removeObserver(this);
    if (DesktopVpnShutdown.isDesktop) {
      unawaited(DesktopVpnShutdown.shutdown(ref));
    }
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (!DesktopVpnShutdown.isDesktop) return;
    if (state == AppLifecycleState.detached) {
      unawaited(DesktopVpnShutdown.shutdown(ref));
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(vpnConnectionWatchdogProvider);
    return widget.child;
  }
}
