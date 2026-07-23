import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/platform/linux_admin.dart';
import '../core/platform/platform_support.dart';
import '../core/platform/windows_admin.dart';
import '../core/vpn/vpn_mode.dart';
import '../l10n/app_localizations.dart';
import '../providers/vpn_providers.dart';

/// TUN 模式需要管理员 / root 时弹出。
Future<void> showTunAdminRequiredDialog(
  BuildContext context,
  WidgetRef ref,
) async {
  final l10n = context.l10n;

  await showDialog<void>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(l10n.tunRequiresAdmin),
      content: Text(l10n.tunRequiresAdminDetail),
      actions: [
        TextButton(
          onPressed: () {
            ref.read(vpnModeProvider.notifier).setMode(VpnMode.systemProxy);
            Navigator.pop(ctx);
          },
          child: Text(l10n.useSystemProxy),
        ),
        TextButton(
          onPressed: () => Navigator.pop(ctx),
          child: Text(l10n.cancel),
        ),
        if (Platform.isWindows)
          FilledButton(
            onPressed: () async {
              Navigator.pop(ctx);
              final ok = await WindowsAdmin.restartAsAdmin();
              if (!ok && context.mounted) {
                ScaffoldMessenger.of(context).showSnackBar(
                  SnackBar(content: Text(l10n.adminRestartFailed)),
                );
              }
            },
            child: Text(l10n.restartAsAdmin),
          ),
      ],
    ),
  );
}

/// 选择 TUN 或连接前检查；非管理员则弹窗并返回 false。
Future<bool> ensureAdminForTunMode(
  BuildContext context,
  WidgetRef ref,
  VpnMode mode,
) async {
  if (mode != VpnMode.tun) return true;
  if (Platform.isAndroid || Platform.isIOS) return true;
  if (Platform.isWindows) {
    if (await WindowsAdmin.isRunningAsAdmin()) return true;
    if (!context.mounted) return false;
    await showTunAdminRequiredDialog(context, ref);
    return false;
  }
  if (Platform.isLinux && PlatformSupport.supportsTunMode) {
    if (await LinuxAdmin.isRoot()) return true;
    if (!context.mounted) return false;
    await showTunAdminRequiredDialog(context, ref);
    return false;
  }
  return true;
}
