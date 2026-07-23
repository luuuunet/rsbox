/// 私有仓库内的客户端密钥（stdash / g5-client 均为 private）。
/// 管理后台：https://tmt.stardash.xyz → 客户端对接 → 自研 App API
class AppSecrets {
  const AppSecrets();

  /// App Key（请求头 X-App-Key）
  final String appKey = 'xtQUUJ05gAiURIeYBn1HaGx4u3yAXgHkzPtIH6VRWvNQLmWA';

  /// 面板根地址（与 [AppConfig.panelBaseUrl] 默认值一致）
  final String panelBaseUrl = 'https://tmt.stardash.xyz';

  /// App API 前缀
  final String apiPrefix = '/api/v1/app';

  String get apiBaseUrl => '$panelBaseUrl$apiPrefix';
}

const appSecrets = AppSecrets();
