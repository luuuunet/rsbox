import 'dart:io';

/// 探测 VPN 隧道是否仍可用（轻量 HTTP 204）。
abstract final class VpnHealthMonitor {
  static const checkTimeout = Duration(seconds: 8);
  static const probeUri = 'http://www.gstatic.com/generate_204';

  /// [proxyPort] 非空时走本地 mixed 代理（系统代理模式）；TUN 传 null。
  static Future<bool> checkConnectivity({int? proxyPort}) async {
    HttpClient? client;
    try {
      client = HttpClient();
      if (proxyPort != null) {
        client.findProxy = (_) => 'PROXY 127.0.0.1:$proxyPort';
      }
      client.connectionTimeout = checkTimeout;
      final req = await client
          .getUrl(Uri.parse(probeUri))
          .timeout(checkTimeout);
      final resp = await req.close().timeout(checkTimeout);
      if (resp.statusCode < 200 || resp.statusCode >= 500) {
        return false;
      }
      await resp.drain().timeout(checkTimeout);
      return true;
    } catch (_) {
      return false;
    } finally {
      client?.close(force: true);
    }
  }
}
