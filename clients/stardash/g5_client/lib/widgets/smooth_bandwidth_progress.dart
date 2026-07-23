import 'package:flutter/material.dart';

import '../core/vpn/bandwidth_format.dart';
import '../theme/g5_theme_extension.dart';

/// 测速进度条：平滑动画 + 实时 Mbps。
class SmoothBandwidthProgress extends StatefulWidget {
  const SmoothBandwidthProgress({
    super.key,
    required this.progress,
    this.liveRateMbps,
    this.label,
  });

  /// 目标进度 0~1。
  final double progress;
  /// 实时速率 MB/s（展示时 ×8 为 Mbps）。
  final double? liveRateMbps;
  final String? label;

  @override
  State<SmoothBandwidthProgress> createState() =>
      _SmoothBandwidthProgressState();
}

class _SmoothBandwidthProgressState extends State<SmoothBandwidthProgress>
    with SingleTickerProviderStateMixin {
  late AnimationController _controller;
  Animation<double> _progressAnim = AlwaysStoppedAnimation(0);
  double _displayRate = 0;
  bool _hasRate = false;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 420),
    );
    _progressAnim = AlwaysStoppedAnimation(widget.progress.clamp(0.0, 1.0));
  }

  @override
  void didUpdateWidget(SmoothBandwidthProgress oldWidget) {
    super.didUpdateWidget(oldWidget);
    final from = _progressAnim.value;
    final to = widget.progress.clamp(0.0, 1.0);
    if ((to - from).abs() > 0.0001) {
      _progressAnim = Tween<double>(begin: from, end: to).animate(
        CurvedAnimation(parent: _controller, curve: Curves.easeOutCubic),
      );
      _controller.forward(from: 0);
    }
    final rate = widget.liveRateMbps;
    if (rate != null && rate > 0) {
      final targetMbps = rate * 8;
      _displayRate = _hasRate ? _displayRate * 0.55 + targetMbps * 0.45 : targetMbps;
      _hasRate = true;
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.g5;
    final speedText = _hasRate
        ? BandwidthFormat.fromMbpsValue(_displayRate)
        : '—';

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            if (widget.label != null)
              Expanded(
                child: Text(
                  widget.label!,
                  style: Theme.of(context).textTheme.bodySmall,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              )
            else
              const Spacer(),
            Text(
              speedText,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: c.primary,
                    fontWeight: FontWeight.w700,
                    letterSpacing: 0.2,
                  ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        AnimatedBuilder(
          animation: _progressAnim,
          builder: (context, _) {
            final value = _progressAnim.value.clamp(0.0, 1.0);
            return LayoutBuilder(
              builder: (context, constraints) {
                final width = constraints.maxWidth;
                return Stack(
                  clipBehavior: Clip.none,
                  children: [
                    Container(
                      height: 8,
                      decoration: BoxDecoration(
                        borderRadius: BorderRadius.circular(999),
                        color: c.primary.withValues(alpha: 0.1),
                      ),
                    ),
                    AnimatedContainer(
                      duration: const Duration(milliseconds: 420),
                      curve: Curves.easeOutCubic,
                      width: width * value,
                      height: 8,
                      decoration: BoxDecoration(
                        borderRadius: BorderRadius.circular(999),
                        gradient: LinearGradient(
                          colors: [
                            c.primary.withValues(alpha: 0.85),
                            c.primary,
                          ],
                        ),
                        boxShadow: [
                          BoxShadow(
                            color: c.primary.withValues(alpha: 0.35),
                            blurRadius: 8,
                            offset: const Offset(0, 1),
                          ),
                        ],
                      ),
                    ),
                  ],
                );
              },
            );
          },
        ),
      ],
    );
  }
}
