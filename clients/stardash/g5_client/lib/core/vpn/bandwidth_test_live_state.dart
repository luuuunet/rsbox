/// 测速进行中的 UI 状态（进度 + 实时速率 MB/s）。
class BandwidthTestLiveState {
  const BandwidthTestLiveState({
    this.progress = 0,
    this.liveRateMbps,
  });

  final double progress;
  final double? liveRateMbps;
}
