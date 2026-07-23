import 'dart:io';

/// Windows 管理员权限检测与 UAC 提权重启。
abstract final class WindowsAdmin {
  static Future<bool> isRunningAsAdmin() async {
    if (!Platform.isWindows) return false;
    final result = await Process.run('powershell', [
      '-NoProfile',
      '-Command',
      '([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)',
    ]);
    return result.stdout.toString().trim() == 'True';
  }

  /// 以管理员身份重新启动当前客户端（UAC 弹窗）。
  static Future<bool> restartAsAdmin() async {
    if (!Platform.isWindows) return false;
    final exe = Platform.resolvedExecutable;
    final escaped = exe.replaceAll("'", "''");
    final result = await Process.run('powershell', [
      '-NoProfile',
      '-Command',
      "Start-Process -FilePath '$escaped' -Verb RunAs",
    ]);
    if (result.exitCode == 0) {
      exit(0);
    }
    return false;
  }
}
