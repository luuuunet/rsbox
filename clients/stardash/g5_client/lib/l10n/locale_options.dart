import 'package:flutter/material.dart';

/// 支持的语言列表。
abstract final class LocaleOptions {
  static const prefKey = 'app_locale';

  static const all = <LocaleOption>[
    LocaleOption(code: 'zh', locale: Locale('zh'), nativeName: '简体中文'),
    LocaleOption(code: 'zh_TW', locale: Locale('zh', 'TW'), nativeName: '繁體中文'),
    LocaleOption(code: 'en', locale: Locale('en'), nativeName: 'English'),
    LocaleOption(code: 'ja', locale: Locale('ja'), nativeName: '日本語'),
    LocaleOption(code: 'ko', locale: Locale('ko'), nativeName: '한국어'),
    LocaleOption(code: 'es', locale: Locale('es'), nativeName: 'Español'),
    LocaleOption(code: 'fr', locale: Locale('fr'), nativeName: 'Français'),
  ];

  static LocaleOption? byCode(String? code) {
    if (code == null) return null;
    for (final o in all) {
      if (o.code == code) return o;
    }
    return null;
  }

  static LocaleOption? byLocale(Locale? locale) {
    if (locale == null) return null;
    for (final o in all) {
      if (o.locale.languageCode == locale.languageCode &&
          o.locale.countryCode == locale.countryCode) {
        return o;
      }
    }
    return all.first;
  }

  static List<Locale> get supportedLocales =>
      all.map((e) => e.locale).toList();
}

class LocaleOption {
  const LocaleOption({
    required this.code,
    required this.locale,
    required this.nativeName,
  });

  final String code;
  final Locale locale;
  final String nativeName;
}
