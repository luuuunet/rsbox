import 'dart:io';

import 'app_secrets.dart';

/// 面板与 App API 配置。
class AppConfig {
  AppConfig._();

  static const String panelBaseUrl = String.fromEnvironment(
    'PANEL_URL',
    defaultValue: 'https://tmt.stardash.xyz',
  );

  static const String apiPrefix = '/api/v1/app';

  static String get apiBaseUrl => '$panelBaseUrl$apiPrefix';

  static Uri get panelUri => Uri.parse(panelBaseUrl);

  /// 当本地 DNS 被污染（如解析到 10.0.0.1）时，直连 Cloudflare 边缘 IP。
  static const String panelFallbackIps = String.fromEnvironment(
    'PANEL_FALLBACK_IPS',
    defaultValue: '104.21.59.241,172.67.185.115',
  );

  static List<InternetAddress> get panelFallbackAddresses {
    return panelFallbackIps
        .split(',')
        .map((s) => s.trim())
        .where((s) => s.isNotEmpty)
        .map(InternetAddress.tryParse)
        .whereType<InternetAddress>()
        .toList();
  }

  static String get appKey => appSecrets.appKey;

  static const String appName = '星驰';

  static String get deviceName {
    if (Platform.isAndroid) return '星驰 Android';
    if (Platform.isIOS) return '星驰 iOS';
    if (Platform.isMacOS) return '星驰 macOS';
    if (Platform.isLinux) return '星驰 Linux';
    return '星驰 Desktop';
  }
}
