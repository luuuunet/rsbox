class SubscribeInfo {
  SubscribeInfo({required this.urls});

  factory SubscribeInfo.fromJson(Map<String, dynamic> json) {
    final raw = json['subscribe_urls'] as Map<String, dynamic>? ?? {};
    return SubscribeInfo(
      urls: raw.map((k, v) => MapEntry(k, v as String)),
    );
  }

  final Map<String, String> urls;

  String? get singbox => urls['singbox'];

  String? get clash => urls['clash'];

  String? get defaultUrl => urls['default'];
}

/// 解析订阅响应头 Subscription-Userinfo
class SubscriptionUserinfo {
  SubscriptionUserinfo({
    this.upload = 0,
    this.download = 0,
    this.total = 0,
    this.expire = 0,
  });

  factory SubscriptionUserinfo.parse(String header) {
    final map = <String, int>{};
    for (final part in header.split(';')) {
      final kv = part.trim().split('=');
      if (kv.length == 2) {
        map[kv[0].trim()] = int.tryParse(kv[1].trim()) ?? 0;
      }
    }
    return SubscriptionUserinfo(
      upload: map['upload'] ?? 0,
      download: map['download'] ?? 0,
      total: map['total'] ?? 0,
      expire: map['expire'] ?? 0,
    );
  }

  final int upload;
  final int download;
  final int total;
  final int expire;
}
