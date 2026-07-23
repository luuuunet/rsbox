import 'dart:io';

/// Linux root / privilege checks (TUN requires root).
class LinuxAdmin {
  LinuxAdmin._();

  static Future<bool> isRoot() async {
    if (!Platform.isLinux) return false;
    try {
      final result = await Process.run('id', ['-u']);
      return result.exitCode == 0 && '${result.stdout}'.trim() == '0';
    } catch (_) {
      return false;
    }
  }
}
