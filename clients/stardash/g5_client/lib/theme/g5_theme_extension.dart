import 'package:flutter/material.dart';

/// 客户端粘土主题色板（浅色 / 深色）。
@immutable
class G5ThemeExtension extends ThemeExtension<G5ThemeExtension> {
  const G5ThemeExtension({
    required this.brightness,
    required this.bg,
    required this.bgElevated,
    required this.surface,
    required this.surfaceSecondary,
    required this.surfaceHigh,
    required this.separator,
    required this.clayBase,
    required this.clayLightShadow,
    required this.clayDarkShadow,
    required this.cardFill,
    required this.glassBorder,
    required this.primary,
    required this.neonPurple,
    required this.textPrimary,
    required this.textSecondary,
    required this.textDim,
    required this.danger,
    required this.warning,
    required this.success,
    required this.backgroundGradient,
    this.backgroundGradientStops,
    this.backgroundGlow,
  });

  final Brightness brightness;
  final Color bg;
  final Color bgElevated;
  final Color surface;
  final Color surfaceSecondary;
  final Color surfaceHigh;
  final Color separator;
  final Color clayBase;
  final Color clayLightShadow;
  final Color clayDarkShadow;
  final Color cardFill;
  final Color glassBorder;
  final Color primary;
  final Color neonPurple;
  final Color textPrimary;
  final Color textSecondary;
  final Color textDim;
  final Color danger;
  final Color warning;
  final Color success;
  final List<Color> backgroundGradient;
  final List<double>? backgroundGradientStops;
  /// 背景氛围光（仅浅色紫色渐变使用）。
  final List<BackgroundGlow>? backgroundGlow;

  bool get isLight => brightness == Brightness.light;

  static const dark = G5ThemeExtension(
    brightness: Brightness.dark,
    bg: Color(0xFF3A3D4E),
    bgElevated: Color(0xFF404456),
    surface: Color(0xFF444756),
    surfaceSecondary: Color(0xFF3E4152),
    surfaceHigh: Color(0xFF4C5062),
    separator: Color(0xFF353848),
    clayBase: Color(0xFF444756),
    clayLightShadow: Color(0x38FFFFFF),
    clayDarkShadow: Color(0x52000000),
    cardFill: Color(0xFF444756),
    glassBorder: Color(0x18FFFFFF),
    primary: Color(0xFF8B9DFF),
    neonPurple: Color(0xFFC4B5FD),
    textPrimary: Color(0xFFF1F2F6),
    textSecondary: Color(0xFFB8BCC8),
    textDim: Color(0xFF8B90A0),
    danger: Color(0xFFFCA5A5),
    warning: Color(0xFFFCD34D),
    success: Color(0xFF86EFAC),
    backgroundGradient: [
      Color(0xFF3E4152),
      Color(0xFF3A3D4E),
      Color(0xFF363948),
    ],
  );

  /// 浅紫渐变 + 磨砂粘土卡片。
  static const light = G5ThemeExtension(
    brightness: Brightness.light,
    bg: Color(0xFFEDE9FE),
    bgElevated: Color(0xFFF5F3FF),
    surface: Color(0xFFFDFBFF),
    surfaceSecondary: Color(0xFFF3EEFF),
    surfaceHigh: Color(0xFFFFFFFF),
    separator: Color(0xFFE4D9FF),
    clayBase: Color(0xFFF9F6FF),
    clayLightShadow: Color(0xCCFFFFFF),
    clayDarkShadow: Color(0x2E7C3AED),
    cardFill: Color(0xFFFCFAFF),
    glassBorder: Color(0x55A78BFA),
    primary: Color(0xFF7C3AED),
    neonPurple: Color(0xFF9333EA),
    textPrimary: Color(0xFF1E1B4B),
    textSecondary: Color(0xFF4C4687),
    textDim: Color(0xFF8B85B1),
    danger: Color(0xFFE11D48),
    warning: Color(0xFFD97706),
    success: Color(0xFF059669),
    backgroundGradient: [
      Color(0xFFC4B5FD),
      Color(0xFFDDD6FE),
      Color(0xFFEDE9FE),
      Color(0xFFF5F3FF),
      Color(0xFFFAF5FF),
    ],
    backgroundGradientStops: [0.0, 0.28, 0.55, 0.82, 1.0],
    backgroundGlow: [
      BackgroundGlow(
        alignment: Alignment(-0.85, -0.75),
        radius: 0.55,
        color: Color(0x66A855F7),
      ),
      BackgroundGlow(
        alignment: Alignment(0.9, -0.35),
        radius: 0.42,
        color: Color(0x55818CF8),
      ),
      BackgroundGlow(
        alignment: Alignment(-0.2, 0.95),
        radius: 0.48,
        color: Color(0x44C084FC),
      ),
    ],
  );

  @override
  G5ThemeExtension copyWith({
    Color? bg,
    Color? primary,
  }) {
    return G5ThemeExtension(
      brightness: brightness,
      bg: bg ?? this.bg,
      bgElevated: bgElevated,
      surface: surface,
      surfaceSecondary: surfaceSecondary,
      surfaceHigh: surfaceHigh,
      separator: separator,
      clayBase: clayBase,
      clayLightShadow: clayLightShadow,
      clayDarkShadow: clayDarkShadow,
      cardFill: cardFill,
      glassBorder: glassBorder,
      primary: primary ?? this.primary,
      neonPurple: neonPurple,
      textPrimary: textPrimary,
      textSecondary: textSecondary,
      textDim: textDim,
      danger: danger,
      warning: warning,
      success: success,
      backgroundGradient: backgroundGradient,
      backgroundGradientStops: backgroundGradientStops,
      backgroundGlow: backgroundGlow,
    );
  }

  @override
  G5ThemeExtension lerp(ThemeExtension<G5ThemeExtension>? other, double t) {
    if (other is! G5ThemeExtension) return this;
    return t < 0.5 ? this : other;
  }
}

/// 背景柔光斑。
@immutable
class BackgroundGlow {
  const BackgroundGlow({
    required this.alignment,
    required this.radius,
    required this.color,
  });

  final Alignment alignment;
  final double radius;
  final Color color;
}

extension G5ThemeContext on BuildContext {
  G5ThemeExtension get g5 =>
      Theme.of(this).extension<G5ThemeExtension>() ?? G5ThemeExtension.dark;
}
