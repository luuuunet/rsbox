import 'dart:io';

/// Thrown when no free local port is available for sing-box mixed inbound.
const kPortUnavailable = 'PORT_UNAVAILABLE';

/// Windows 端口占用检测与释放。
class WindowsPortHelper {
  WindowsPortHelper._();

  static const defaultMixedPort = 7890;
  static const mixedPortRangeStart = 17890;
  static const mixedPortRangeEnd = 17999;
  static const defaultSpeedTestPort = 17891;

  static ServerSocket? _mixedReservation;
  static final Set<int> _trackedVpnPids = {};
  static bool _connectPrepared = false;

  static void trackVpnCorePid(int pid) {
    if (pid > 0) _trackedVpnPids.add(pid);
  }

  static void untrackVpnCorePid(int pid) {
    _trackedVpnPids.remove(pid);
  }

  /// 连接前清理本客户端遗留进程并等待端口释放。
  static Future<void> prepareForVpnConnect({bool force = false}) async {
    if (!Platform.isWindows) return;
    await releaseMixedReservation();
    await killTrackedVpnCores();
    if (!_connectPrepared || force) {
      await releaseStaleMixedPorts();
      await releasePort(defaultMixedPort, onlySingbox: true);
      await Future<void>.delayed(const Duration(milliseconds: 500));
      _connectPrepared = true;
    }
  }

  /// 启动时清理 mixed 端口段上遗留的本客户端 VPN 内核。
  static Future<void> releaseStaleMixedPorts() async {
    if (!Platform.isWindows) return;
    for (var port = mixedPortRangeStart; port <= mixedPortRangeEnd; port++) {
      if (await isPortListening(port)) {
        await releasePort(port, onlySingbox: true);
      }
    }
  }

  /// 是否为本地端口绑定冲突。
  static bool isPortBindError(Object error) {
    final msg = error.toString().toLowerCase();
    if (msg.contains('端口被占用') ||
        msg.contains('port unavailable') ||
        msg.contains(kPortUnavailable.toLowerCase()) ||
        msg.contains('10048') ||
        msg.contains('只允许使用一次')) {
      return true;
    }
    return msg.contains('bind') &&
        (msg.contains('only one usage') ||
            msg.contains('address already in use') ||
            msg.contains('permitted') ||
            msg.contains('listen tcp'));
  }

  static RegExp _portPattern(int port) =>
      RegExp(':$port(?!\\d)|\\[$port\\](?!\\d)');

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

  /// 选定端口并短暂占用，直到 sing-box 启动前释放，避免竞态。
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

  /// 端口是否已有进程在监听。
  static Future<bool> isPortListening(int port) async {
    if (!Platform.isWindows) return false;
    final pids = await _listeningPids(port);
    return pids.isNotEmpty;
  }

  /// 本地 loopback 端口是否可绑定（未被占用）。
  static Future<bool> canBind(int port) async {
    if (!Platform.isWindows) return false;
    if (await isPortListening(port)) return false;
    try {
      final socket = await ServerSocket.bind(
        InternetAddress.loopbackIPv4,
        port,
        shared: false,
      );
      await socket.close();
      await Future<void>.delayed(const Duration(milliseconds: 80));
      return true;
    } on SocketException {
      return false;
    }
  }

  /// 为 mixed 入站选择端口：优先 17890+，避开 Clash 常用的 7890。
  static Future<int> findMixedListenPort({Set<int> exclude = const {}}) async {
    if (!Platform.isWindows) return defaultMixedPort;

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
      if (!await _isVpnCorePid(pid)) return true;
    }
    return false;
  }

  /// 节点测速用临时端口。
  static Future<int> findSpeedTestPort() async {
    if (!Platform.isWindows) return defaultSpeedTestPort;

    if (await canBind(defaultSpeedTestPort)) return defaultSpeedTestPort;

    await releasePort(defaultSpeedTestPort, onlySingbox: true);
    await Future<void>.delayed(const Duration(milliseconds: 250));
    if (await canBind(defaultSpeedTestPort)) return defaultSpeedTestPort;

    for (var port = defaultSpeedTestPort + 1; port <= mixedPortRangeEnd; port++) {
      if (await canBind(port)) return port;
    }

    throw StateError(kPortUnavailable);
  }

  /// 结束监听 [port] 的进程；默认仅结束 rsbox / sing-box，避免误杀 Clash 等其它代理。
  static Future<void> releasePort(int port, {bool onlySingbox = true}) async {
    if (!Platform.isWindows) return;

    final pids = await _listeningPids(port);
    for (final pid in pids) {
      if (onlySingbox && !await _isVpnCorePid(pid)) continue;
      untrackVpnCorePid(pid);
      await Process.run('taskkill', ['/F', '/PID', '$pid']);
    }
    if (pids.isNotEmpty) {
      await Future<void>.delayed(const Duration(milliseconds: 450));
    }
  }

  /// 结束本客户端启动的 VPN 内核子进程（仅已登记 PID）。
  static Future<void> killTrackedVpnCores() async {
    if (!Platform.isWindows) return;
    if (_trackedVpnPids.isEmpty) return;

    final pids = Set<int>.from(_trackedVpnPids);
    for (final pid in pids) {
      await Process.run('taskkill', ['/F', '/PID', '$pid', '/T']);
      _trackedVpnPids.remove(pid);
    }
    await Future<void>.delayed(const Duration(milliseconds: 300));
  }

  /// @deprecated 使用 [killTrackedVpnCores]
  static Future<void> killOrphanVpnCores() => killTrackedVpnCores();

  /// @deprecated 使用 [killTrackedVpnCores]
  static Future<void> killOrphanSingBox() => killTrackedVpnCores();

  static Future<bool> _isVpnCorePid(int pid) async {
    final result = await Process.run(
      'tasklist',
      ['/FI', 'PID eq $pid', '/FO', 'CSV', '/NH'],
    );
    final name = '${result.stdout}'.toLowerCase();
    return name.contains('sing-box') || name.contains('rsbox');
  }

  static Future<Set<int>> _listeningPids(int port) async {
    final result = await Process.run('netstat', ['-ano']);
    if (result.exitCode != 0) return {};

    final pattern = _portPattern(port);
    final pids = <int>{};
    for (final line in '${result.stdout}'.split('\n')) {
      if (!line.contains('LISTENING') || !pattern.hasMatch(line)) continue;
      final parts = line.trim().split(RegExp(r'\s+'));
      if (parts.length < 5) continue;
      final pid = int.tryParse(parts.last);
      if (pid != null && pid > 0) pids.add(pid);
    }
    return pids;
  }
}
