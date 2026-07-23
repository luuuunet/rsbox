/// 复制本文件为 `app_secrets.dart` 并填入后台「客户端对接 → 自研 App API」中的 App Key。
class AppSecrets {
  const AppSecrets();

  /// 留空表示不发送 X-App-Key（仅当面板未配置 Key 时可用）。
  final String appKey = 'YOUR_APP_KEY_HERE';
}

const appSecrets = AppSecrets();
