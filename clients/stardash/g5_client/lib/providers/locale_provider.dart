import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../l10n/locale_options.dart';

class LocaleNotifier extends StateNotifier<Locale> {
  LocaleNotifier() : super(const Locale('zh')) {
    _load();
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    final code = prefs.getString(LocaleOptions.prefKey);
    final option = LocaleOptions.byCode(code);
    if (option != null) {
      state = option.locale;
    }
  }

  Future<void> setLocaleCode(String code) async {
    final option = LocaleOptions.byCode(code);
    if (option == null) return;
    state = option.locale;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(LocaleOptions.prefKey, code);
  }
}

final localeProvider = StateNotifierProvider<LocaleNotifier, Locale>(
  (ref) => LocaleNotifier(),
);
