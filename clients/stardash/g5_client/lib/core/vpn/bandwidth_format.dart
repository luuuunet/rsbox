/// 将测速结果格式化为 Mbps 展示。
abstract final class BandwidthFormat {
  /// [megabytesPerSec] 兆字节/秒，展示时 ×8 转为 Mbps。
  static String fromDownloadRate(double megabytesPerSec) {
    return fromMbpsValue(megabytesPerSec * 8);
  }

  static String fromMbpsValue(double mbps) {
    if (!mbps.isFinite || mbps <= 0) return '—';
    if (mbps >= 100) return '${mbps.round()} Mbps';
    return '${mbps.toStringAsFixed(1)} Mbps';
  }
}
