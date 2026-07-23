import 'package:flutter/material.dart';

/// 兼容旧代码的静态色（默认深色）；新代码请用 [BuildContext.g5]。
abstract final class AppColors {
  static const bg = Color(0xFF3A3D4E);
  static const bgElevated = Color(0xFF404456);
  static const surface = Color(0xFF444756);
  static const surfaceSecondary = Color(0xFF3E4152);
  static const surfaceHigh = Color(0xFF4C5062);
  static const separator = Color(0xFF353848);
  static const clayBase = Color(0xFF444756);
  static const clayLightShadow = Color(0x38FFFFFF);
  static const clayDarkShadow = Color(0x52000000);
  static const cardFill = Color(0xFF444756);
  static const glassBorder = Color(0x18FFFFFF);
  static const primary = Color(0xFF8B9DFF);
  static const neonPurple = Color(0xFFC4B5FD);
  static const textPrimary = Color(0xFFF1F2F6);
  static const textSecondary = Color(0xFFB8BCC8);
  static const textDim = Color(0xFF8B90A0);
  static const danger = Color(0xFFFCA5A5);
  static const warning = Color(0xFFFCD34D);
  static const success = Color(0xFF86EFAC);

  static const primaryGradient = LinearGradient(
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
    colors: [Color(0xFF9CA8FF), Color(0xFFC4B5FD)],
  );
}
