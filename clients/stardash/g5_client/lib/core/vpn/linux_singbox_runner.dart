import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'linux_port_helper.dart';

/// 定位 / 释放 sing-box 二进制，并管理 Linux 子进程。
class LinuxSingboxRunner {
  LinuxSingboxRunner();

  Process? _process;
  String? _configPath;
  int? _activeMixedPort;
  final _stderrLines = <String>[];

  void Function(int exitCode, String detail)? onProcessExit;

  bool get isRunning => _process != null;

  Future<Directory> ensureBinaries() async {
    if (!Platform.isLinux) {
      throw UnsupportedError('sing-box Linux runner 仅支持 Linux');
    }

    final execDir = File(Platform.resolvedExecutable).parent;
    final candidates = <Directory>[
      Directory('${execDir.path}/binaries/linux'),
      Directory('${(await getApplicationSupportDirectory()).path}/bin'),
    ];

    for (final dir in candidates) {
      final exe = File('${dir.path}/sing-box');
      if (await exe.exists()) {
        await _ensureExecutable(exe);
        return dir;
      }
    }

    final target = candidates.last;
    await target.create(recursive: true);
    final exe = File('${target.path}/sing-box');
    await _extractAsset('assets/binaries/linux/sing-box', exe);
    await _ensureExecutable(exe);
    return target;
  }

  Future<void> _ensureExecutable(File exe) async {
    if (!await exe.exists()) return;
    await Process.run('chmod', ['+x', exe.path]);
  }

  Future<void> _extractAsset(String assetPath, File target) async {
    if (await target.exists()) return;
    final data = await rootBundle.load(assetPath);
    await target.writeAsBytes(
      data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes),
      flush: true,
    );
    await _ensureExecutable(target);
  }

  Future<void> start({
    required Directory binDir,
    required String configJson,
    int? mixedPort,
  }) async {
    await stop(mixedPort: mixedPort);
    await LinuxPortHelper.releaseMixedReservation();
    await LinuxPortHelper.killTrackedVpnCores();
    if (mixedPort != null) {
      await LinuxPortHelper.releasePort(mixedPort, onlySingbox: true);
    }
    _activeMixedPort = mixedPort;

    final configDir = Directory(
      '${(await getApplicationSupportDirectory()).path}/config',
    );
    await configDir.create(recursive: true);
    _configPath = '${configDir.path}/runtime.json';
    await File(_configPath!).writeAsString(configJson, flush: true);

    final exe = '${binDir.path}/sing-box';
    await _validateConfig(exe, _configPath!);

    _stderrLines.clear();
    _process = await Process.start(
      exe,
      ['run', '-c', _configPath!],
      workingDirectory: binDir.path,
      mode: ProcessStartMode.normal,
    );
    LinuxPortHelper.trackVpnCorePid(_process!.pid);

    _process!.stderr.transform(utf8.decoder).listen((chunk) {
      for (final line in chunk.split('\n')) {
        final trimmed = line.trim();
        if (trimmed.isNotEmpty) {
          _stderrLines.add(trimmed);
          // ignore: avoid_print
          print('[sing-box] $trimmed');
        }
      }
    });

    _watchProcessExit(_process!);
    await _waitUntilRunning();
  }

  void _watchProcessExit(Process proc) {
    proc.exitCode.then((code) {
      if (_process != proc) return;
      LinuxPortHelper.untrackVpnCorePid(proc.pid);
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
    if (raw.contains('unknown field')) {
      return 'sing-box 配置无效（含未知字段）。请在客户端点「刷新节点」后重试。';
    }
    if (raw.contains('legacy tun address') ||
        raw.contains('ENABLE_DEPRECATED_TUN_ADDRESS')) {
      return 'TUN 配置不兼容当前 sing-box 版本，请改用「系统代理」模式。';
    }
    if (raw.contains('outbound not found') ||
        raw.contains('default outbound not found')) {
      return '所选节点在订阅中不存在，请点「刷新节点」后重新选择。';
    }
    if (raw.contains('bind') &&
        (raw.contains('Only one usage') ||
            raw.contains('address already in use'))) {
      return '本地代理端口被占用，请关闭其它代理软件或稍后重试。';
    }
    if (raw.contains('bind') ||
        (raw.contains('listen') && raw.contains('address already in use'))) {
      return '本地代理端口被占用且无法自动切换，请关闭其它代理软件后重试。';
    }
    if (raw.contains('permission denied') || raw.contains('operation not permitted')) {
      return 'TUN 模式需要 root 权限，请使用 sudo 运行客户端或改用系统代理。';
    }
    final line = raw.split('\n').where((l) => l.trim().isNotEmpty).lastOrNull;
    return line ?? raw;
  }

  Future<void> _waitUntilRunning() async {
    final proc = _process;
    if (proc == null) return;
    try {
      await Future.any([
        proc.exitCode.then((code) {
          final detail = _stderrLines.isNotEmpty
              ? _friendlyError(_stderrLines.join('\n'))
              : 'exit $code';
          throw StateError('sing-box 启动失败: $detail');
        }),
        Future<void>.delayed(const Duration(milliseconds: 900)),
      ]);
    } on StateError {
      rethrow;
    }
  }

  Future<void> stop({int? mixedPort}) async {
    final port = mixedPort ?? _activeMixedPort;
    final proc = _process;
    _process = null;
    _activeMixedPort = null;
    if (proc != null) {
      LinuxPortHelper.untrackVpnCorePid(proc.pid);
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
    }
    await LinuxPortHelper.killTrackedVpnCores();
    if (port != null) {
      await LinuxPortHelper.releasePort(port, onlySingbox: true);
    }
  }
}
