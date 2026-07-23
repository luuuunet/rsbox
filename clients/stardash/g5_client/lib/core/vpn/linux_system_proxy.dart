import 'dart:io';

/// 通过 gsettings 设置 GNOME 系统代理（HTTP/HTTPS → mixed 端口）。
class LinuxSystemProxy {
  static String? _savedMode;

  static Future<void> enable(String host, int port) async {
    if (!Platform.isLinux) return;
    if (!await _hasGsettings()) return;

    await _saveCurrent();
    await _runGsettings(['set', 'org.gnome.system.proxy', 'mode', 'manual']);
    await _runGsettings([
      'set',
      'org.gnome.system.proxy.http',
      'host',
      host,
    ]);
    await _runGsettings([
      'set',
      'org.gnome.system.proxy.http',
      'port',
      '$port',
    ]);
    await _runGsettings([
      'set',
      'org.gnome.system.proxy.https',
      'host',
      host,
    ]);
    await _runGsettings([
      'set',
      'org.gnome.system.proxy.https',
      'port',
      '$port',
    ]);
    await _runGsettings([
      'set',
      'org.gnome.system.proxy',
      'ignore-hosts',
      "['localhost', '127.0.0.1', '::1']",
    ]);
    // SOCKS 部分应用会读 socks host
    await _runGsettings([
      'set',
      'org.gnome.system.proxy.socks',
      'host',
      host,
    ]);
    await _runGsettings([
      'set',
      'org.gnome.system.proxy.socks',
      'port',
      '$port',
    ]);
  }

  static Future<void> disable() async {
    if (!Platform.isLinux) return;
    if (!await _hasGsettings()) return;

    if (_savedMode != null) {
      await _runGsettings([
        'set',
        'org.gnome.system.proxy',
        'mode',
        _savedMode!,
      ]);
    } else {
      await _runGsettings(['set', 'org.gnome.system.proxy', 'mode', 'none']);
    }
    _savedMode = null;
  }

  static Future<bool> _hasGsettings() async {
    final which = await Process.run('which', ['gsettings']);
    return which.exitCode == 0;
  }

  static Future<void> _saveCurrent() async {
    final result = await Process.run('gsettings', [
      'get',
      'org.gnome.system.proxy',
      'mode',
    ]);
    if (result.exitCode == 0) {
      _savedMode = '${result.stdout}'.trim();
    }
  }

  static Future<void> _runGsettings(List<String> args) async {
    final result = await Process.run('gsettings', args);
    if (result.exitCode != 0) {
      throw StateError('gsettings 失败: ${result.stderr}');
    }
  }
}
