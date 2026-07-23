import 'dart:async';
import 'dart:io';

/// 测速过程快照（进度 0~1，速率为 MB/s）。
class BandwidthTestSnapshot {
  const BandwidthTestSnapshot({
    required this.progress,
    this.downloadRateMbps,
    this.isComplete = false,
  });

  final double progress;
  final double? downloadRateMbps;
  final bool isComplete;
}

/// 通过 HTTP 代理或系统/VPN 路由下载测速，返回 MB/s（兆字节/秒）。
abstract final class VpnBandwidthTest {
  static const downloadBytes = 6000000;
  static const maxDuration = Duration(seconds: 10);
  static const _tickInterval = Duration(milliseconds: 50);
  static const _instantWindow = Duration(milliseconds: 700);

  static Uri get downloadUri => Uri.parse(
        'https://speed.cloudflare.com/__down?bytes=$downloadBytes',
      );

  /// [proxyPort] 非空时走本地 mixed 代理；TUN/移动端 VPN 已连接时传 null 即可。
  static Future<double?> measure({
    int? proxyPort,
    void Function(BandwidthTestSnapshot snapshot)? onProgress,
  }) async {
    final client = HttpClient();
    try {
      if (proxyPort != null) {
        client.findProxy = (_) => 'PROXY 127.0.0.1:$proxyPort';
      }
      client.connectionTimeout = const Duration(seconds: 15);
      return await _streamDownloadRate(client, onProgress: onProgress);
    } finally {
      client.close(force: true);
    }
  }

  static Future<double?> _streamDownloadRate(
    HttpClient client, {
    void Function(BandwidthTestSnapshot snapshot)? onProgress,
  }) async {
    final sw = Stopwatch()..start();
    var totalBytes = 0;
    Timer? tickTimer;
    final byteMarks = <({int bytes, Duration at})>[];

    void notify({required double progress, double? rate, bool complete = false}) {
      onProgress?.call(
        BandwidthTestSnapshot(
          progress: progress.clamp(0.0, 1.0),
          downloadRateMbps: rate,
          isComplete: complete,
        ),
      );
    }

    double? instantRate() {
      if (byteMarks.length < 2) return null;
      final latest = byteMarks.last;
      final cutoff = latest.at - _instantWindow;
      var anchor = byteMarks.first;
      for (final mark in byteMarks) {
        if (mark.at >= cutoff) {
          anchor = mark;
          break;
        }
      }
      final deltaBytes = latest.bytes - anchor.bytes;
      final deltaMs = latest.at.inMilliseconds - anchor.at.inMilliseconds;
      if (deltaMs < 120 || deltaBytes <= 0) return null;
      return deltaBytes / (deltaMs / 1000.0) / 1000000;
    }

    void tick() {
      final elapsed = sw.elapsed;
      if (elapsed >= maxDuration) return;
      final progress = elapsed.inMilliseconds / maxDuration.inMilliseconds;
      notify(progress: progress, rate: instantRate());
    }

    try {
      notify(progress: 0);
      tickTimer = Timer.periodic(_tickInterval, (_) => tick());

      final req = await client.getUrl(downloadUri);
      final resp = await req.close().timeout(const Duration(seconds: 15));
      if (resp.statusCode < 200 || resp.statusCode >= 400) return null;

      await for (final chunk in resp.timeout(maxDuration)) {
        totalBytes += chunk.length;
        byteMarks.add((bytes: totalBytes, at: sw.elapsed));
        if (sw.elapsed >= maxDuration) break;
      }
      sw.stop();
      tickTimer.cancel();

      if (totalBytes < 200000) return null;
      final seconds = sw.elapsedMilliseconds / 1000.0;
      if (seconds <= 0) return null;
      final result = totalBytes / seconds / 1000000;
      notify(progress: 1, rate: result, complete: true);
      return result;
    } catch (_) {
      return null;
    } finally {
      tickTimer?.cancel();
    }
  }
}
