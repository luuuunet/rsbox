import 'dart:io';

import 'app_secrets.dart';

/// 面板与 App API 配置。
class AppConfig {
  AppConfig._();

  static const String panelBaseUrl = String.fromEnvironment(
    'PANEL_URL',
    defaultValue: 'https://g5.lulunet.cc',
  );

  static const String apiPrefix = '/api/v1/app';

  static String get apiBaseUrl => '$panelBaseUrl$apiPrefix';

  static String get appKey => appSecrets.appKey;

  static const String appName = 'G5 VPN';

  static String get deviceName {
    if (Platform.isAndroid) return 'G5 Android Client';
    if (Platform.isIOS) return 'G5 iOS Client';
    if (Platform.isMacOS) return 'G5 macOS Client';
    if (Platform.isLinux) return 'G5 Linux Client';
    return 'G5 Desktop Client';
  }
}
