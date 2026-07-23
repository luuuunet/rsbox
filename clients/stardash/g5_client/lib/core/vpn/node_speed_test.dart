import 'dart:async';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../models/proxy_node.dart';
import 'linux_port_helper.dart';
import 'linux_singbox_runner.dart';
import 'singbox_config_builder.dart';
import 'singbox_test_route.dart';
import 'vpn_bandwidth_test.dart';
import 'windows_port_helper.dart';
import 'windows_singbox_runner.dart';
import 'windows_vpn_kernel.dart';

/// 节点延迟测速结果 sentinel。
abstract final class NodeLatency {
  static const int testing = -2;
  static const int timeout = -1;
}

/// 节点下载测速结果 sentinel（MB/s，展示时 ×8 为 Mbps）。
abstract final class NodeBandwidth {
  static const double testing = -2;
  static const double failed = -1;
}

class NodeSpeedTest {
  static const _udpProxyTypes = {
    'hysteria',
    'hysteria2',
    'rsq',
    'tuic',
    'wireguard',
  };

  static const maxConcurrentLatencyTests = 3;

  static bool get _desktopSingbox => Platform.isWindows || Platform.isLinux;

  /// TCP / ping 不通时，桌面端回退到 sing-box 代理实测（适合 Hysteria2）。
  static Future<int> measureLatency(
    ProxyNode node, {
    Map<String, dynamic>? baseConfig,
    SingboxTestRoute? reuseRoute,
    WindowsVpnKernel? windowsKernel,
  }) async {
    if (!node.isConnectable) return NodeLatency.timeout;

    final isUdpProxy = _udpProxyTypes.contains(node.type.toLowerCase());

    if (!isUdpProxy) {
      final tcp = await _tcpLatency(node.server, node.port);
      if (tcp != null) return tcp;
    }

    final ping = await _pingLatency(node.server);
    if (ping != null) return ping;

    if (reuseRoute != null &&
        reuseRoute.matchesNode(node.tag) &&
        reuseRoute.proxyPort != null) {
      final proxy = await _proxyLatencyOnPort(reuseRoute.proxyPort!);
      if (proxy != null) return proxy;
    }

    if (baseConfig != null) {
      final proxy = await _proxyLatency(
        node,
        baseConfig,
        windowsKernel: windowsKernel,
      );
      if (proxy != null) return proxy;
    }

    return NodeLatency.timeout;
  }

  /// 经 sing-box 代理下载测速，返回 MB/s。
  static Future<double?> measureBandwidth(
    ProxyNode node, {
    Map<String, dynamic>? baseConfig,
    SingboxTestRoute? reuseRoute,
    WindowsVpnKernel? windowsKernel,
    void Function(BandwidthTestSnapshot snapshot)? onProgress,
  }) async {
    if (!node.isConnectable) return null;

    if (reuseRoute != null && reuseRoute.matchesNode(node.tag)) {
      return VpnBandwidthTest.measure(
        proxyPort: reuseRoute.proxyPort,
        onProgress: onProgress,
      );
    }

    if (!_desktopSingbox || baseConfig == null) {
      return null;
    }

    final session = await _SingboxProxySession.start(
      node,
      baseConfig,
      windowsKernel: windowsKernel,
    );
    if (session == null) return null;
    try {
      return await VpnBandwidthTest.measure(
        proxyPort: session.port,
        onProgress: onProgress,
      );
    } finally {
      await session.dispose();
    }
  }

  static Future<Map<String, int>> measureAll(
    List<ProxyNode> nodes, {
    Map<String, dynamic>? baseConfig,
    WindowsVpnKernel? windowsKernel,
    void Function(int done, int total, String tag, int ms)? onProgress,
  }) async {
    final result = <String, int>{};
    var done = 0;
    final total = nodes.length;

    await _runConcurrent(
      total,
      maxConcurrentLatencyTests,
      (index) async {
        final node = nodes[index];
        final ms = await measureLatency(
          node,
          baseConfig: baseConfig,
          windowsKernel: windowsKernel,
        );
        result[node.tag] = ms;
        done++;
        onProgress?.call(done, total, node.tag, ms);
      },
    );
    return result;
  }

  static Future<void> _runConcurrent(
    int total,
    int concurrency,
    Future<void> Function(int index) worker,
  ) async {
    if (total <= 0) return;
    var next = 0;
    final workers = List.generate(
      concurrency.clamp(1, total),
      (_) async {
        while (true) {
          final index = next++;
          if (index >= total) break;
          await worker(index);
        }
      },
    );
    await Future.wait(workers);
  }

  static Future<int?> _tcpLatency(String host, int port) async {
    final sw = Stopwatch()..start();
    try {
      final socket = await Socket.connect(
        host,
        port,
        timeout: const Duration(milliseconds: 1500),
      );
      await socket.close();
      sw.stop();
      return sw.elapsedMilliseconds;
    } catch (_) {
      return null;
    }
  }

  static Future<int?> _pingLatency(String host) async {
    if (!Platform.isWindows && !Platform.isLinux) return null;
    try {
      final List<String> args;
      if (Platform.isWindows) {
        args = ['-n', '1', '-w', '2000', host];
      } else {
        args = ['-c', '1', '-W', '2', host];
      }
      final result = await Process.run(
        'ping',
        args,
        runInShell: Platform.isWindows,
      );
      final out = '${result.stdout}${result.stderr}';
      if (RegExp(r'时间<1\s*ms', caseSensitive: false).hasMatch(out)) {
        return 1;
      }
      final match = RegExp(
        r'(?:time|时间)[=<]\s*(\d+(?:\.\d+)?)\s*ms',
        caseSensitive: false,
      ).firstMatch(out);
      if (match != null) {
        final ms = double.tryParse(match.group(1)!);
        if (ms != null) return ms.ceil();
      }
      if (out.contains('TTL=') ||
          out.contains('TTL =') ||
          out.contains('ttl=')) {
        return 999;
      }
    } catch (_) {}
    return null;
  }

  static Future<int?> _proxyLatencyOnPort(int port) async {
    HttpClient? client;
    try {
      final sw = Stopwatch()..start();
      client = HttpClient();
      client.findProxy = (_) => 'PROXY 127.0.0.1:$port';
      client.connectionTimeout = const Duration(seconds: 8);

      final req = await client.getUrl(
        Uri.parse('http://www.gstatic.com/generate_204'),
      );
      final resp = await req.close().timeout(const Duration(seconds: 8));
      if (resp.statusCode >= 200 && resp.statusCode < 500) {
        await resp.drain();
        sw.stop();
        return sw.elapsedMilliseconds;
      }
    } catch (_) {
      return null;
    } finally {
      client?.close(force: true);
    }
    return null;
  }

  static Future<int?> _proxyLatency(
    ProxyNode node,
    Map<String, dynamic> baseConfig, {
    WindowsVpnKernel? windowsKernel,
  }) async {
    if (!_desktopSingbox) return null;

    final session = await _SingboxProxySession.start(
      node,
      baseConfig,
      windowsKernel: windowsKernel,
    );
    if (session == null) return null;

    HttpClient? client;
    try {
      client = HttpClient();
      client.findProxy = (_) => 'PROXY 127.0.0.1:${session.port}';
      client.connectionTimeout = const Duration(seconds: 8);

      final sw = Stopwatch()..start();
      final req = await client.getUrl(
        Uri.parse('http://www.gstatic.com/generate_204'),
      );
      final resp = await req.close().timeout(const Duration(seconds: 8));
      if (resp.statusCode >= 200 && resp.statusCode < 500) {
        await resp.drain();
        sw.stop();
        return sw.elapsedMilliseconds;
      }
    } catch (_) {
      return null;
    } finally {
      client?.close(force: true);
      await session.dispose();
    }
    return null;
  }
}

class _SingboxProxySession {
  _SingboxProxySession(this._proc, this.port);

  final Process _proc;
  final int port;

  static Future<_SingboxProxySession?> start(
    ProxyNode node,
    Map<String, dynamic> baseConfig, {
    WindowsVpnKernel? windowsKernel,
  }) async {
    if (!NodeSpeedTest._desktopSingbox) return null;

    final kernel = windowsKernel ?? WindowsVpnKernel.singbox;

    final port = Platform.isWindows
        ? await WindowsPortHelper.findSpeedTestPort()
        : await LinuxPortHelper.findSpeedTestPort();

    try {
      if (Platform.isWindows) {
        await WindowsPortHelper.releasePort(port);
      } else {
        await LinuxPortHelper.releasePort(port);
      }

      final configJson = SingboxConfigBuilder.buildSpeedTestConfig(
        baseConfig: baseConfig,
        nodeTag: node.tag,
        listenPort: port,
        windowsKernel: Platform.isWindows ? kernel : null,
      );

      final binDir = Platform.isWindows
          ? await WindowsSingboxRunner(kernel: kernel).ensureBinaries()
          : await LinuxSingboxRunner().ensureBinaries();
      final exe = Platform.isWindows
          ? '${binDir.path}\\${kernel.exeName}'
          : '${binDir.path}/sing-box';
      final support = await getApplicationSupportDirectory();
      final configPath = Platform.isWindows
          ? '${support.path}\\speedtest-${node.tag.hashCode}.json'
          : '${support.path}/speedtest-${node.tag.hashCode}.json';
      await File(configPath).writeAsString(configJson, flush: true);

      final check = await Process.run(
        exe,
        ['check', '-c', configPath],
        workingDirectory: binDir.path,
      );
      if (check.exitCode != 0) return null;

      final proc = await Process.start(
        exe,
        ['run', '-c', configPath],
        workingDirectory: binDir.path,
      );
      if (Platform.isWindows) {
        WindowsPortHelper.trackVpnCorePid(proc.pid);
      } else if (Platform.isLinux) {
        LinuxPortHelper.trackVpnCorePid(proc.pid);
      }

      await Future<void>.delayed(const Duration(milliseconds: 900));
      return _SingboxProxySession(proc, port);
    } catch (_) {
      return null;
    }
  }

  Future<void> dispose() async {
    if (Platform.isWindows) {
      WindowsPortHelper.untrackVpnCorePid(_proc.pid);
    } else if (Platform.isLinux) {
      LinuxPortHelper.untrackVpnCorePid(_proc.pid);
    }
    try {
      _proc.kill(ProcessSignal.sigterm);
    } catch (_) {}
    await _proc.exitCode.timeout(
      const Duration(seconds: 2),
      onTimeout: () {
        try {
          _proc.kill(ProcessSignal.sigkill);
        } catch (_) {}
        return -1;
      },
    );
    if (Platform.isWindows) {
      await WindowsPortHelper.releasePort(port);
    } else {
      await LinuxPortHelper.releasePort(port);
    }
  }
}
