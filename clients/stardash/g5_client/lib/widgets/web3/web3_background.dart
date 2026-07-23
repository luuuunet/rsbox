import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../theme/g5_theme_extension.dart';

/// 主题背景：深色静态粘土 / 浅色 Gemini 式动态紫渐变。
class Web3Background extends StatelessWidget {
  const Web3Background({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    final theme = context.g5;
    if (theme.isLight) {
      return _GeminiAnimatedBackground(theme: theme, child: child);
    }
    return _StaticBackground(theme: theme, child: child);
  }
}

class _StaticBackground extends StatelessWidget {
  const _StaticBackground({required this.theme, required this.child});

  final G5ThemeExtension theme;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Stack(
      fit: StackFit.expand,
      children: [
        DecoratedBox(
          decoration: BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.topLeft,
              end: Alignment.bottomRight,
              colors: theme.backgroundGradient,
              stops: theme.backgroundGradientStops,
            ),
          ),
        ),
        child,
      ],
    );
  }
}

class _GeminiAnimatedBackground extends StatefulWidget {
  const _GeminiAnimatedBackground({
    required this.theme,
    required this.child,
  });

  final G5ThemeExtension theme;
  final Widget child;

  @override
  State<_GeminiAnimatedBackground> createState() =>
      _GeminiAnimatedBackgroundState();
}

class _GeminiAnimatedBackgroundState extends State<_GeminiAnimatedBackground>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: const Duration(seconds: 24),
    )..repeat();
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return RepaintBoundary(
      child: Stack(
        fit: StackFit.expand,
        children: [
          AnimatedBuilder(
            animation: _controller,
            builder: (context, _) {
              return CustomPaint(
                painter: _GeminiMeshPainter(
                  t: _controller.value,
                  theme: widget.theme,
                ),
                child: const SizedBox.expand(),
              );
            },
          ),
          widget.child,
        ],
      ),
    );
  }
}

class _GeminiMeshPainter extends CustomPainter {
  _GeminiMeshPainter({required this.t, required this.theme});

  final double t;
  final G5ThemeExtension theme;

  static const _blobs = [
    _BlobSpec(
      phase: 0.00,
      speed: 1.00,
      orbitX: 0.38,
      orbitY: 0.32,
      radius: 0.72,
      color: Color(0xFFA855F7),
    ),
    _BlobSpec(
      phase: 0.27,
      speed: 0.82,
      orbitX: 0.42,
      orbitY: 0.36,
      radius: 0.65,
      color: Color(0xFF818CF8),
    ),
    _BlobSpec(
      phase: 0.53,
      speed: 0.68,
      orbitX: 0.34,
      orbitY: 0.40,
      radius: 0.58,
      color: Color(0xFFC084FC),
    ),
    _BlobSpec(
      phase: 0.71,
      speed: 0.91,
      orbitX: 0.30,
      orbitY: 0.28,
      radius: 0.52,
      color: Color(0xFF6366F1),
    ),
    _BlobSpec(
      phase: 0.15,
      speed: 0.75,
      orbitX: 0.36,
      orbitY: 0.34,
      radius: 0.48,
      color: Color(0xFFE879F9),
    ),
  ];

  @override
  void paint(Canvas canvas, Size size) {
    if (size.isEmpty) return;

    final rect = Offset.zero & size;
    final pulse = (math.sin(t * math.pi * 2) + 1) * 0.5;

    final basePaint = Paint()
      ..shader = LinearGradient(
        begin: Alignment(
          -0.8 + 0.25 * math.sin(t * math.pi * 2),
          -1,
        ),
        end: Alignment(
          0.9 - 0.2 * math.cos(t * math.pi * 2),
          1,
        ),
        colors: [
          Color.lerp(const Color(0xFFFAF5FF), const Color(0xFFEDE9FE), pulse)!,
          Color.lerp(const Color(0xFFDDD6FE), const Color(0xFFC4B5FD), pulse)!,
          Color.lerp(const Color(0xFFF5F3FF), const Color(0xFFE9D5FF), 1 - pulse)!,
        ],
        stops: const [0.0, 0.52, 1.0],
      ).createShader(rect);
    canvas.drawRect(rect, basePaint);

    final centerX = size.width * 0.5;
    final centerY = size.height * 0.48;
    final base = size.shortestSide;

    for (final blob in _blobs) {
      final angle = t * math.pi * 2 * blob.speed + blob.phase * math.pi * 2;
      final cx = centerX + math.cos(angle) * base * blob.orbitX;
      final cy = centerY + math.sin(angle * 0.87 + blob.phase) * base * blob.orbitY;
      final breathe = 1 + 0.06 * math.sin(t * math.pi * 4 + blob.phase * 6);
      final radius = base * blob.radius * breathe;

      final blobRect = Rect.fromCircle(center: Offset(cx, cy), radius: radius);
      final blobPaint = Paint()
        ..shader = RadialGradient(
          colors: [
            blob.color.withValues(alpha: 0.55),
            blob.color.withValues(alpha: 0.28),
            blob.color.withValues(alpha: 0),
          ],
          stops: const [0.0, 0.42, 1.0],
        ).createShader(blobRect);
      canvas.drawCircle(Offset(cx, cy), radius, blobPaint);
    }

    final wash = Paint()
      ..shader = LinearGradient(
        begin: Alignment.topCenter,
        end: Alignment.bottomCenter,
        colors: [
          Colors.white.withValues(alpha: 0.34),
          Colors.white.withValues(alpha: 0.08),
          theme.neonPurple.withValues(alpha: 0.05),
        ],
        stops: const [0.0, 0.55, 1.0],
      ).createShader(rect);
    canvas.drawRect(rect, wash);
  }

  @override
  bool shouldRepaint(covariant _GeminiMeshPainter oldDelegate) {
    return oldDelegate.t != t;
  }
}

class _BlobSpec {
  const _BlobSpec({
    required this.phase,
    required this.speed,
    required this.orbitX,
    required this.orbitY,
    required this.radius,
    required this.color,
  });

  final double phase;
  final double speed;
  final double orbitX;
  final double orbitY;
  final double radius;
  final Color color;
}
