import 'dart:io';

import 'windows_port_helper.dart';

export 'windows_port_helper.dart' show kPortUnavailable;

/// Linux 端口占用检测与 sing-box 进程清理。
class LinuxPortHelper {
  LinuxPortHelper._();

  static const defaultMixedPort = WindowsPortHelper.defaultMixedPort;
  static const mixedPortRangeStart = WindowsPortHelper.mixedPortRangeStart;
  static const mixedPortRangeEnd = WindowsPortHelper.mixedPortRangeEnd;
  static const defaultSpeedTestPort = WindowsPortHelper.defaultSpeedTestPort;

  static final Set<int> _trackedVpnPids = {};
  static bool _connectPrepared = false;

  static void trackVpnCorePid(int pid) {
    if (pid > 0) _trackedVpnPids.add(pid);
  }

  static void untrackVpnCorePid(int pid) {
    _trackedVpnPids.remove(pid);
  }

  static Future<void> prepareForVpnConnect({bool force = false}) async {
    if (!Platform.isLinux) return;
    await releaseMixedReservation();
    await killTrackedVpnCores();
    if (!_connectPrepared || force) {
      await releaseStaleMixedPorts();
      await releasePort(defaultMixedPort, onlySingbox: true);
      await Future<void>.delayed(const Duration(milliseconds: 500));
      _connectPrepared = true;
    }
  }

  static Future<void> releaseStaleMixedPorts() async {
    if (!Platform.isLinux) return;
    for (var port = mixedPortRangeStart; port <= mixedPortRangeEnd; port++) {
      if (await isPortListening(port)) {
        await releasePort(port, onlySingbox: true);
      }
    }
  }

  static Future<void> killTrackedVpnCores() async {
    if (!Platform.isLinux) return;
    if (_trackedVpnPids.isEmpty) return;

    final pids = Set<int>.from(_trackedVpnPids);
    for (final pid in pids) {
      try {
        Process.killPid(pid, ProcessSignal.sigterm);
      } catch (_) {}
      _trackedVpnPids.remove(pid);
    }
    await Future<void>.delayed(const Duration(milliseconds: 300));
  }

  static bool isPortBindError(Object error) =>
      WindowsPortHelper.isPortBindError(error);

  static ServerSocket? _mixedReservation;

  static Future<void> releaseMixedReservation() async {
    final socket = _mixedReservation;
    _mixedReservation = null;
    if (socket != null) {
      try {
        await socket.close();
      } catch (_) {}
      await Future<void>.delayed(const Duration(milliseconds: 120));
    }
  }

  static Future<int> findAndReserveMixedPort({Set<int> exclude = const {}}) async {
    final port = await findMixedListenPort(exclude: exclude);
    await releaseMixedReservation();
    _mixedReservation = await ServerSocket.bind(
      InternetAddress.loopbackIPv4,
      port,
      shared: false,
    );
    return port;
  }

  static Future<bool> isPortListening(int port) async {
    if (!Platform.isLinux) return false;
    final pids = await _listeningPids(port);
    return pids.isNotEmpty;
  }

  static Future<bool> canBind(int port) async {
    if (!Platform.isLinux) return false;
    if (await isPortListening(port)) return false;
    try {
      final socket =
          await ServerSocket.bind(InternetAddress.loopbackIPv4, port);
      await socket.close();
      await Future<void>.delayed(const Duration(milliseconds: 80));
      return true;
    } on SocketException {
      return false;
    }
  }

  static Future<int> findMixedListenPort({Set<int> exclude = const {}}) async {
    if (!Platform.isLinux) return defaultMixedPort;

    await prepareForVpnConnect();

    for (var port = mixedPortRangeStart; port <= mixedPortRangeEnd; port++) {
      if (exclude.contains(port)) continue;
      if (await _tryMixedPort(port)) return port;
    }

    if (!exclude.contains(defaultMixedPort) &&
        await _tryMixedPort(defaultMixedPort)) {
      return defaultMixedPort;
    }

    throw StateError(kPortUnavailable);
  }

  static Future<bool> _tryMixedPort(int port) async {
    if (await _portHeldByNonSingbox(port)) return false;

    if (await isPortListening(port)) {
      await releasePort(port, onlySingbox: true);
      await Future<void>.delayed(const Duration(milliseconds: 450));
    }

    return canBind(port);
  }

  static Future<bool> _portHeldByNonSingbox(int port) async {
    final pids = await _listeningPids(port);
    for (final pid in pids) {
      if (!await _isSingBoxPid(pid)) return true;
    }
    return false;
  }

  static Future<int> findSpeedTestPort() async {
    if (!Platform.isLinux) return defaultSpeedTestPort;

    if (await canBind(defaultSpeedTestPort)) return defaultSpeedTestPort;

    await releasePort(defaultSpeedTestPort, onlySingbox: true);
    await Future<void>.delayed(const Duration(milliseconds: 250));
    if (await canBind(defaultSpeedTestPort)) return defaultSpeedTestPort;

    for (var port = defaultSpeedTestPort + 1; port <= mixedPortRangeEnd; port++) {
      if (await canBind(port)) return port;
    }

    throw StateError(kPortUnavailable);
  }

  static Future<void> releasePort(int port, {bool onlySingbox = true}) async {
    if (!Platform.isLinux) return;

    final pids = await _listeningPids(port);
    for (final pid in pids) {
      if (onlySingbox && !await _isSingBoxPid(pid)) continue;
      untrackVpnCorePid(pid);
      try {
        Process.killPid(pid, ProcessSignal.sigterm);
      } catch (_) {}
    }
    if (pids.isNotEmpty) {
      await Future<void>.delayed(const Duration(milliseconds: 450));
    }
  }

  static Future<void> killOrphanSingBox() => killTrackedVpnCores();

  static Future<bool> _isSingBoxPid(int pid) async {
    try {
      final link = File('/proc/$pid/exe');
      if (!await link.exists()) return false;
      final target = await link.resolveSymbolicLinks();
      return target.toLowerCase().contains('sing-box');
    } catch (_) {
      return false;
    }
  }

  static Future<Set<int>> _listeningPids(int port) async {
    final result = await Process.run('ss', ['-tlnp']);
    if (result.exitCode != 0) return {};

    final pattern = RegExp(':$port(?!\\d)');
    final pids = <int>{};
    for (final line in '${result.stdout}'.split('\n')) {
      if (!line.contains('LISTEN') || !pattern.hasMatch(line)) continue;
      for (final match in RegExp(r'pid=(\d+)').allMatches(line)) {
        final pid = int.tryParse(match.group(1)!);
        if (pid != null && pid > 0) pids.add(pid);
      }
    }
    return pids;
  }
}
