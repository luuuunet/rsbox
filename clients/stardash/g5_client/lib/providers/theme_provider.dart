import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// 主题偏好：跟随系统 / 浅色 / 深色。
enum ThemePreference { system, light, dark }

class ThemePreferenceNotifier extends StateNotifier<ThemePreference> {
  ThemePreferenceNotifier() : super(ThemePreference.dark) {
    _load();
  }

  static const prefKey = 'theme_preference';

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    final raw = prefs.getString(prefKey);
    state = ThemePreference.values.firstWhere(
      (e) => e.name == raw,
      orElse: () => ThemePreference.dark,
    );
  }

  Future<void> setPreference(ThemePreference value) async {
    state = value;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(prefKey, value.name);
  }
}

final themePreferenceProvider =
    StateNotifierProvider<ThemePreferenceNotifier, ThemePreference>(
  (ref) => ThemePreferenceNotifier(),
);

final themeModeProvider = Provider<ThemeMode>((ref) {
  return switch (ref.watch(themePreferenceProvider)) {
    ThemePreference.system => ThemeMode.system,
    ThemePreference.light => ThemeMode.light,
    ThemePreference.dark => ThemeMode.dark,
  };
});
