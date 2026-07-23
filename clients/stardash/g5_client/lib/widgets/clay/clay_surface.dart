import 'package:flutter/material.dart';

import '../../theme/g5_theme_extension.dart';

/// Claymorphism 凹凸装饰：凸面（浮起）与凹面（按压槽）。
abstract final class ClayDecoration {
  static BoxDecoration convex({
    Color? base,
    required G5ThemeExtension theme,
    double radius = 16,
    double depth = 5,
    bool circle = false,
    Color? accent,
  }) {
    final color = base ?? theme.clayBase;
    final isLight = theme.isLight;
    final highlight = Color.lerp(
      color,
      isLight ? const Color(0xFFFFFFFF) : Colors.white,
      isLight ? 0.42 : 0.1,
    )!;
    final mid = Color.lerp(
      color,
      isLight ? theme.neonPurple : color,
      isLight ? 0.04 : 0,
    )!;
    final shade = Color.lerp(
      color,
      isLight ? theme.primary : Colors.black,
      isLight ? 0.07 : 0.1,
    )!;

    return BoxDecoration(
      shape: circle ? BoxShape.circle : BoxShape.rectangle,
      borderRadius: circle ? null : BorderRadius.circular(radius),
      gradient: LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
        colors: [highlight, mid, shade],
        stops: const [0.0, 0.45, 1.0],
      ),
      border: isLight
          ? Border.all(
              color: theme.glassBorder.withValues(alpha: 0.35),
              width: 0.8,
            )
          : null,
      boxShadow: [
        BoxShadow(
          color: theme.clayLightShadow,
          offset: Offset(-depth * 0.65, -depth * 0.65),
          blurRadius: depth * (isLight ? 2.2 : 1.8),
          spreadRadius: isLight ? 1 : 0.5,
        ),
        BoxShadow(
          color: theme.clayDarkShadow,
          offset: Offset(depth * 0.55, depth * 0.65),
          blurRadius: depth * (isLight ? 2.4 : 1.8),
          spreadRadius: isLight ? 0 : 0.5,
        ),
        if (isLight)
          BoxShadow(
            color: theme.primary.withValues(alpha: 0.08),
            blurRadius: depth * 4,
            spreadRadius: 1,
          ),
        if (accent != null)
          BoxShadow(
            color: accent.withValues(alpha: isLight ? 0.22 : 0.18),
            blurRadius: depth * 3,
            spreadRadius: -2,
          ),
      ],
    );
  }

  static BoxDecoration concave({
    Color? base,
    required G5ThemeExtension theme,
    double radius = 16,
    bool circle = false,
  }) {
    final color = base ?? theme.clayBase;
    final isLight = theme.isLight;
    final innerDark = Color.lerp(
      color,
      isLight ? theme.primary : Colors.black,
      isLight ? 0.09 : 0.16,
    )!;
    final innerLight = Color.lerp(
      color,
      isLight ? Colors.white : Colors.white,
      isLight ? 0.55 : 0.04,
    )!;

    return BoxDecoration(
      shape: circle ? BoxShape.circle : BoxShape.rectangle,
      borderRadius: circle ? null : BorderRadius.circular(radius),
      gradient: LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
        colors: [innerDark, color, innerLight],
        stops: const [0.0, 0.55, 1.0],
      ),
      border: Border.all(
        color: isLight
            ? theme.glassBorder.withValues(alpha: 0.45)
            : Colors.black.withValues(alpha: 0.12),
        width: 0.8,
      ),
      boxShadow: [
        BoxShadow(
          color: isLight
              ? theme.primary.withValues(alpha: 0.12)
              : Colors.black.withValues(alpha: 0.22),
          offset: const Offset(2, 2),
          blurRadius: isLight ? 8 : 4,
          spreadRadius: -1,
        ),
        BoxShadow(
          color: isLight
              ? Colors.white.withValues(alpha: 0.85)
              : Colors.white.withValues(alpha: 0.04),
          offset: const Offset(-2, -2),
          blurRadius: isLight ? 6 : 3,
          spreadRadius: -1,
        ),
      ],
    );
  }
}

enum ClayStyle { convex, concave }

/// 通用粘土容器。
class ClaySurface extends StatelessWidget {
  const ClaySurface({
    super.key,
    required this.child,
    this.style = ClayStyle.convex,
    this.padding,
    this.margin,
    this.borderRadius = 16,
    this.circle = false,
    this.color,
    this.accent,
    this.depth = 5,
    this.onTap,
    this.width,
    this.height,
  });

  final Widget child;
  final ClayStyle style;
  final EdgeInsetsGeometry? padding;
  final EdgeInsetsGeometry? margin;
  final double borderRadius;
  final bool circle;
  final Color? color;
  final Color? accent;
  final double depth;
  final VoidCallback? onTap;
  final double? width;
  final double? height;

  @override
  Widget build(BuildContext context) {
    final theme = context.g5;
    final base = color ?? theme.clayBase;
    final decoration = style == ClayStyle.convex
        ? ClayDecoration.convex(
            base: base,
            theme: theme,
            radius: borderRadius,
            depth: depth,
            circle: circle,
            accent: accent,
          )
        : ClayDecoration.concave(
            base: base,
            theme: theme,
            radius: borderRadius,
            circle: circle,
          );

    Widget box = AnimatedContainer(
      duration: const Duration(milliseconds: 220),
      width: width,
      height: height,
      decoration: decoration,
      child: padding != null ? Padding(padding: padding!, child: child) : child,
    );

    if (margin != null) {
      box = Padding(padding: margin!, child: box);
    }

    if (onTap == null) return box;

    return GestureDetector(
      onTap: onTap,
      child: box,
    );
  }
}
