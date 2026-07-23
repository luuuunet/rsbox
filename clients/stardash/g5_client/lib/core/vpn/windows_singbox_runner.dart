import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'windows_port_helper.dart';
import '../platform/windows_admin.dart';
import 'vpn_mode.dart';
import 'windows_vpn_kernel.dart';

/// 定位 / 释放 Windows VPN 内核（rsbox 或 sing-box）与 wintun.dll，并管理子进程。
class WindowsSingboxRunner {
  WindowsSingboxRunner({WindowsVpnKernel kernel = WindowsVpnKernel.singbox})
      : _kernel = kernel;

  WindowsVpnKernel _kernel;

  WindowsVpnKernel get kernel => _kernel;

  set kernel(WindowsVpnKernel value) => _kernel = value;

  Process? _process;
  String? _configPath;
  int? _activeMixedPort;
  final _stderrLines = <String>[];
  VpnMode? _activeMode;

  /// VPN 内核意外退出时回调（exitCode, 友好错误信息）。
  void Function(int exitCode, String detail)? onProcessExit;

  bool get isRunning => _process != null;

  Future<Directory> ensureBinaries() async {
    if (!Platform.isWindows) {
      throw UnsupportedError('Windows VPN runner 仅支持 Windows 桌面端');
    }

    final exeDir = File(Platform.resolvedExecutable).parent;
    final candidates = <Directory>[
      Directory('${exeDir.path}\\binaries\\windows'),
      Directory('${(await getApplicationSupportDirectory()).path}\\bin'),
    ];

    for (final dir in candidates) {
      if (await _hasCompleteBinaries(dir)) {
        await _refreshKernelIfStale(dir, candidates);
        return dir;
      }
    }

    final target = candidates.last;
    await target.create(recursive: true);

    final targetExe = File('${target.path}\\${_kernel.exeName}');
    await _syncKernelExe(targetExe, candidates);

    final targetWintun = File('${target.path}\\wintun.dll');
    if (!await targetWintun.exists()) {
      final existingWintun = await _findExistingFile(candidates, 'wintun.dll');
      if (existingWintun != null) {
        await existingWintun.copy(targetWintun.path);
      } else {
        await _extractAsset(
          'assets/binaries/windows/wintun.dll',
          targetWintun,
          minBytes: 32 * 1024,
        );
      }
    }

    return target;
  }

  Future<void> _extractAsset(
    String assetPath,
    File target, {
    required int minBytes,
  }) async {
    if (await target.exists() && await target.length() >= minBytes) {
      return;
    }
    final bytes = await _loadAssetBytes(assetPath, minBytes: minBytes);
    await target.writeAsBytes(bytes, flush: true);
  }

  Future<bool> _hasCompleteBinaries(Directory dir) async {
    final exe = File('${dir.path}\\${_kernel.exeName}');
    final wintun = File('${dir.path}\\wintun.dll');
    if (!await exe.exists() || !await wintun.exists()) {
      return false;
    }
    final exeLen = await exe.length();
    final wintunLen = await wintun.length();
    return exeLen > 1024 * 1024 && wintunLen > 32 * 1024;
  }

  Future<File?> _findExistingFile(
    List<Directory> dirs,
    String name,
  ) async {
    for (final dir in dirs) {
      final file = File('${dir.path}\\$name');
      if (await file.exists() && await file.length() > 0) {
        return file;
      }
    }
    return null;
  }

  Future<void> _refreshKernelIfStale(
    Directory dir,
    List<Directory> allCandidates,
  ) async {
    final targetExe = File('${dir.path}\\${_kernel.exeName}');
    if (!await targetExe.exists()) return;
    await _syncKernelExe(targetExe, allCandidates);
  }

  /// Pick the largest on-disk kernel among candidate dirs (dev deploy wins over
  /// stale flutter build copies).
  Future<File?> _findNewestKernelFile(List<Directory> dirs) async {
    File? newest;
    var newestLen = 0;
    for (final dir in dirs) {
      final file = File('${dir.path}\\${_kernel.exeName}');
      if (!await file.exists()) continue;
      final len = await file.length();
      if (len > newestLen && len > 1024 * 1024) {
        newestLen = len;
        newest = file;
      }
    }
    return newest;
  }

  Future<void> _syncKernelExe(
    File targetExe,
    List<Directory> candidates,
  ) async {
    final newest = await _findNewestKernelFile(candidates);
    if (newest != null) {
      if (newest.path != targetExe.path) {
        await newest.copy(targetExe.path);
      }
      return;
    }

    // No kernel on disk yet — extract the bundled asset.
    final bundled = await _loadAssetBytes(
      _kernel.assetPath,
      minBytes: 1024 * 1024,
    );
    if (!await targetExe.exists()) {
      await targetExe.writeAsBytes(bundled, flush: true);
    }
  }

  Future<Uint8List> _loadAssetBytes(
    String assetPath, {
    required int minBytes,
  }) async {
    try {
      final data = await rootBundle.load(assetPath);
      final bytes = data.buffer.asUint8List(
        data.offsetInBytes,
        data.lengthInBytes,
      );
      if (bytes.length < minBytes) {
        throw StateError(
          '内置 $assetPath 无效（文件过小）。'
          '请运行 scripts\\download_singbox.ps1 或放置 ${_kernel.exeName} 后重新编译。',
        );
      }
      return bytes;
    } catch (_) {
      throw StateError(
        '缺少 VPN 组件 $assetPath。'
        '请运行 scripts\\download_singbox.ps1 下载 ${_kernel.displayLabel} 与 wintun，然后重新编译。',
      );
    }
  }

  Future<bool> _checkTunConflict() async {
    try {
      final result = await Process.run(
        'netsh',
        ['interface', 'show', 'interface', 'name=g5-tun'],
      );
      return result.exitCode == 0;
    } catch (_) {
      return false;
    }
  }

  Future<void> _cleanupTunInterface() async {
    try {
      await Process.run(
        'netsh',
        ['interface', 'delete', 'interface', 'name=g5-tun'],
      );
      await Future<void>.delayed(const Duration(milliseconds: 500));
    } catch (_) {}
  }

  Future<void> start({
    required Directory binDir,
    required String configJson,
    int? mixedPort,
    VpnMode? mode,
    WindowsVpnKernel? kernel,
  }) async {
    _activeMode = mode;
    final activeKernel = kernel ?? _kernel;

    if (mode == VpnMode.tun) {
      if (await _checkTunConflict()) {
        await _cleanupTunInterface();
        if (await _checkTunConflict()) {
          throw StateError(
            'TUN 接口已存在，无法启动全局模式。\n\n'
            '解决方案：\n'
            '1. 完全关闭应用后重新打开\n'
            '2. 切换到"系统代理"模式\n'
            '3. 以管理员身份运行应用',
          );
        }
      }
    }

    await stop(mixedPort: mixedPort, mode: mode);
    await WindowsPortHelper.releaseMixedReservation();
    await WindowsPortHelper.killOrphanVpnCores();
    if (mixedPort != null) {
      await WindowsPortHelper.releasePort(mixedPort, onlySingbox: true);
    }
    _activeMixedPort = mixedPort;

    final configDir = Directory(
      '${(await getApplicationSupportDirectory()).path}\\config',
    );
    await configDir.create(recursive: true);
    _configPath = '${configDir.path}\\runtime.json';
    await File(_configPath!).writeAsString(configJson, flush: true);

    final exe = '${binDir.path}\\${activeKernel.exeName}';
    await _validateConfig(exe, _configPath!);

    _stderrLines.clear();
    _process = await Process.start(
      exe,
      ['run', '-c', _configPath!],
      workingDirectory: binDir.path,
      mode: ProcessStartMode.normal,
    );
    WindowsPortHelper.trackVpnCorePid(_process!.pid);

    final prefix = activeKernel.logPrefix;
    _process!.stderr.transform(utf8.decoder).listen((chunk) {
      for (final line in chunk.split('\n')) {
        final trimmed = line.trim();
        if (trimmed.isNotEmpty) {
          _stderrLines.add(trimmed);
          // ignore: avoid_print
          print('[$prefix] $trimmed');
        }
      }
    });

    _watchProcessExit(_process!);
    await _waitUntilRunning();
  }

  void _watchProcessExit(Process proc) {
    proc.exitCode.then((code) {
      if (_process != proc) return;
      WindowsPortHelper.untrackVpnCorePid(proc.pid);
      _process = null;
      final detail = _stderrLines.isNotEmpty
          ? _friendlyError(_stderrLines.join('\n'))
          : '进程已退出 (code $code)';
      onProcessExit?.call(code, detail);
    });
  }

  Future<void> _validateConfig(String exe, String configPath) async {
    final result = await Process.run(exe, ['check', '-c', configPath]);
    if (result.exitCode == 0) return;
    final err = '${result.stderr}${result.stdout}'.trim();
    throw StateError(_friendlyError(err));
  }

  String _friendlyError(String raw) {
    raw = raw.replaceAll(RegExp(r'\x1B\[[0-9;]*m'), '');
    final name = _kernel.displayLabel;
    if (raw.contains('Cannot create a file when that file already exists') ||
        (raw.contains('tun') && raw.contains('configure tun interface'))) {
      return 'TUN 接口冲突，无法启动全局模式。\n\n'
          '解决方案：\n'
          '1. 完全关闭应用后重新打开\n'
          '2. 切换到"系统代理"模式（推荐）\n'
          '3. 以管理员身份运行应用';
    }

    if (raw.contains('Access is denied') && raw.contains('tun')) {
      return '全局模式需要管理员权限。\n\n'
          '请右键选择"以管理员身份运行"，\n'
          '或使用"系统代理"模式。';
    }

    if (raw.contains('legacy DNS servers') ||
        raw.contains('ENABLE_DEPRECATED_LEGACY_DNS_SERVERS')) {
      return '$name DNS 配置不兼容当前版本，请更新客户端后重试。';
    }
    if (raw.contains('missing domain resolver') ||
        raw.contains('ENABLE_DEPRECATED_MISSING_DOMAIN_RESOLVER')) {
      return '$name 缺少 domain_resolver 配置，请更新客户端后重试。';
    }

    if (raw.contains('unknown field')) {
      return '$name 配置无效（含未知字段）。请在客户端点「刷新节点」后重试。';
    }
    if (raw.contains('legacy tun address') ||
        raw.contains('ENABLE_DEPRECATED_TUN_ADDRESS')) {
      return 'TUN 配置不兼容当前 $name 版本，请改用「系统代理」模式。';
    }
    if (raw.contains('outbound not found') ||
        raw.contains('default outbound not found')) {
      return '所选节点在订阅中不存在，请点「刷新节点」后重新选择。';
    }
    if (raw.contains('bind') &&
        (raw.contains('Only one usage') ||
            raw.contains('address already in use') ||
            raw.contains('10048') ||
            raw.contains('只允许使用一次'))) {
      return '本地代理端口被占用，请关闭其它代理软件或稍后重试。';
    }
    final line = raw.split('\n').where((l) => l.trim().isNotEmpty).lastOrNull;
    return line ?? raw;
  }

  Future<void> _waitUntilRunning() async {
    final proc = _process;
    if (proc == null) return;
    final name = _kernel.displayLabel;
    try {
      await Future.any([
        proc.exitCode.then((code) {
          final detail = _stderrLines.isNotEmpty
              ? _friendlyError(_stderrLines.join('\n'))
              : 'exit $code';
          throw StateError('$name 启动失败: $detail');
        }),
        Future<void>.delayed(const Duration(milliseconds: 900)),
      ]);
    } on StateError {
      rethrow;
    }
  }

  Future<void> stop({int? mixedPort, VpnMode? mode}) async {
    final port = mixedPort ?? _activeMixedPort;
    final currentMode = mode ?? _activeMode;
    final proc = _process;
    _process = null;
    _activeMixedPort = null;
    _activeMode = null;

    if (proc != null) {
      WindowsPortHelper.untrackVpnCorePid(proc.pid);
      try {
        proc.kill(ProcessSignal.sigterm);
      } catch (_) {}
      await proc.exitCode.timeout(
        const Duration(seconds: 3),
        onTimeout: () {
          try {
            proc.kill(ProcessSignal.sigkill);
          } catch (_) {}
          return -1;
        },
      );

      if (currentMode == VpnMode.tun) {
        await Future<void>.delayed(const Duration(milliseconds: 1000));
      }
    }

    // 清理僵尸连接和本客户端登记的孤儿进程
    await WindowsPortHelper.killTrackedVpnCores();
    if (port != null) {
      await WindowsPortHelper.releasePort(port, onlySingbox: true);
      // 额外等待确保端口完全释放，清理 CLOSE_WAIT 连接
      await Future<void>.delayed(const Duration(milliseconds: 500));
    }

    if (currentMode == VpnMode.tun) {
      await _cleanupTunInterface();
    }
  }

  static Future<bool> isRunningAsAdmin() => WindowsAdmin.isRunningAsAdmin();
}
