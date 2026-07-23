import 'dart:io';

/// 通过注册表切换 Windows 系统代理（HTTP/SOCKS 混合端口）。
class WindowsSystemProxy {
  static const _regPath =
      r'HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings';

  static String? _savedEnable;
  static String? _savedServer;
  static String? _savedOverride;

  static Future<void> enable(String host, int port) async {
    if (!Platform.isWindows) return;
    await _saveCurrent();
    await _runReg([
      'add',
      _regPath,
      '/v',
      'ProxyEnable',
      '/t',
      'REG_DWORD',
      '/d',
      '1',
      '/f',
    ]);
    await _runReg([
      'add',
      _regPath,
      '/v',
      'ProxyServer',
      '/t',
      'REG_SZ',
      '/d',
      '$host:$port',
      '/f',
    ]);
    await _runReg([
      'add',
      _regPath,
      '/v',
      'ProxyOverride',
      '/t',
      'REG_SZ',
      '/d',
      '<local>',
      '/f',
    ]);
    await _notifySettingsChanged();
  }

  static Future<void> disable() async {
    if (!Platform.isWindows) return;
    if (_savedEnable != null) {
      await _runReg([
        'add',
        _regPath,
        '/v',
        'ProxyEnable',
        '/t',
        'REG_DWORD',
        '/d',
        _savedEnable!,
        '/f',
      ]);
    } else {
      await _runReg([
        'add',
        _regPath,
        '/v',
        'ProxyEnable',
        '/t',
        'REG_DWORD',
        '/d',
        '0',
        '/f',
      ]);
    }
    if (_savedServer != null) {
      await _runReg([
        'add',
        _regPath,
        '/v',
        'ProxyServer',
        '/t',
        'REG_SZ',
        '/d',
        _savedServer!,
        '/f',
      ]);
    }
    if (_savedOverride != null) {
      await _runReg([
        'add',
        _regPath,
        '/v',
        'ProxyOverride',
        '/t',
        'REG_SZ',
        '/d',
        _savedOverride!,
        '/f',
      ]);
    }
    _savedEnable = null;
    _savedServer = null;
    _savedOverride = null;
    await _notifySettingsChanged();
  }

  static Future<void> _saveCurrent() async {
    _savedEnable = await _queryValue('ProxyEnable');
    _savedServer = await _queryValue('ProxyServer');
    _savedOverride = await _queryValue('ProxyOverride');
  }

  static Future<String?> _queryValue(String name) async {
    final result = await Process.run('reg', ['query', _regPath, '/v', name]);
    if (result.exitCode != 0) return null;
    final lines = '${result.stdout}'.split('\n');
    for (final line in lines) {
      if (line.contains(name)) {
        final parts = line.trim().split(RegExp(r'\s{2,}'));
        if (parts.length >= 3) return parts.last.trim();
      }
    }
    return null;
  }

  static Future<void> _runReg(List<String> args) async {
    final result = await Process.run('reg', args);
    if (result.exitCode != 0) {
      throw StateError('reg 失败: ${result.stderr}');
    }
  }

  static Future<void> _notifySettingsChanged() async {
    await Process.run('powershell', [
      '-NoProfile',
      '-Command',
      r'''
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinInet {
  [DllImport("wininet.dll", SetLastError=true)]
  public static extern bool InternetSetOption(IntPtr h, int o, IntPtr b, int l);
}
"@
[WinInet]::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0) | Out-Null
[WinInet]::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0) | Out-Null
''',
    ]);
  }
}
