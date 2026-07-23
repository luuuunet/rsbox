import 'package:flutter/material.dart';

import '../clay/clay_surface.dart';

/// 粘土卡片（凸面浮起，可选凹面）。
class GlassCard extends StatelessWidget {
  const GlassCard({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.all(16),
    this.margin,
    this.borderRadius = 20,
    this.onTap,
    this.color,
    this.glowColor,
    this.inset = false,
    this.depth = 5,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;
  final EdgeInsetsGeometry? margin;
  final double borderRadius;
  final VoidCallback? onTap;
  final Color? color;
  final Color? glowColor;
  final bool inset;
  final double depth;

  @override
  Widget build(BuildContext context) {
    return ClaySurface(
      style: inset ? ClayStyle.concave : ClayStyle.convex,
      padding: padding,
      margin: margin,
      borderRadius: borderRadius,
      color: color,
      accent: glowColor,
      depth: depth,
      onTap: onTap,
      child: child,
    );
  }
}
