/// 国家码解析与国旗展示（与面板 CountryHelper 对齐）。
class CountryFlags {
  CountryFlags._();

  static const _names = <String, String>{
    'US': '美国',
    'HK': '中国香港',
    'TW': '中国台湾',
    'JP': '日本',
    'KR': '韩国',
    'SG': '新加坡',
    'GB': '英国',
    'DE': '德国',
    'FR': '法国',
    'NL': '荷兰',
    'CA': '加拿大',
    'AU': '澳大利亚',
    'IN': '印度',
    'RU': '俄罗斯',
    'TR': '土耳其',
    'BR': '巴西',
    'VN': '越南',
    'TH': '泰国',
    'MY': '马来西亚',
    'PH': '菲律宾',
    'ID': '印度尼西亚',
    'AE': '阿联酋',
    'CH': '瑞士',
    'SE': '瑞典',
    'IT': '意大利',
    'ES': '西班牙',
    'PL': '波兰',
    'UA': '乌克兰',
    'MX': '墨西哥',
    'AR': '阿根廷',
    'NZ': '新西兰',
    'IE': '爱尔兰',
    'FI': '芬兰',
    'NO': '挪威',
    'DK': '丹麦',
    'AT': '奥地利',
    'BE': '比利时',
    'CZ': '捷克',
    'RO': '罗马尼亚',
    'IL': '以色列',
    'SA': '沙特阿拉伯',
    'ZA': '南非',
    'CN': '中国',
  };

  static final _nameToCode = {
    for (final e in _names.entries) e.value: e.key,
    '香港': 'HK',
    '台湾': 'TW',
    '韩国': 'KR',
    '新加坡': 'SG',
    '英国': 'GB',
    '德国': 'DE',
    '法国': 'FR',
    '荷兰': 'NL',
    '加拿大': 'CA',
    '澳大利亚': 'AU',
    '日本': 'JP',
    '美国': 'US',
  };

  static String? normalize(String? code) {
    if (code == null || code.trim().isEmpty) return null;
    final cleaned = code.replaceAll(RegExp(r'[^A-Za-z]'), '').toUpperCase();
    if (cleaned.length < 2) return null;
    return cleaned.substring(0, 2);
  }

  /// 从订阅 outbound 元数据或节点名称推断 ISO 3166-1 alpha-2。
  static String? resolve({String? metaCode, required String tag}) {
    final fromMeta = normalize(metaCode);
    if (fromMeta != null) return fromMeta;

    final upper = tag.toUpperCase();

    // 名称关键词（中文）
    for (final entry in _nameToCode.entries) {
      if (tag.contains(entry.key)) return entry.value;
    }
    for (final entry in _names.entries) {
      if (tag.contains(entry.value)) return entry.key;
    }

    // 英文国名
    const en = {
      'CANADA': 'CA',
      'UNITED STATES': 'US',
      'USA': 'US',
      'JAPAN': 'JP',
      'SINGAPORE': 'SG',
      'GERMANY': 'DE',
      'FRANCE': 'FR',
      'UNITED KINGDOM': 'GB',
      'UK': 'GB',
      'HONG KONG': 'HK',
      'TAIWAN': 'TW',
      'KOREA': 'KR',
      'AUSTRALIA': 'AU',
    };
    for (final entry in en.entries) {
      if (upper.contains(entry.key)) return entry.value;
    }

    // 独立两位码：CA-01、US 节点、[HK]
    final codeMatch = RegExp(r'(?:^|[\s\[\(·\-_])([A-Z]{2})(?:[\s\]\)·\-_\d]|$)').firstMatch(upper);
    if (codeMatch != null) {
      final c = codeMatch.group(1)!;
      if (_names.containsKey(c)) return c;
    }

    return null;
  }

  static String emoji(String? code) {
    final c = normalize(code);
    if (c == null || c.length != 2) return '🌐';
    final units = c.codeUnits;
    if (units.length != 2) return '🌐';
    return String.fromCharCodes([
      0x1F1E6 + units[0] - 65,
      0x1F1E6 + units[1] - 65,
    ]);
  }

  static String name(String? code) {
    final c = normalize(code);
    if (c == null) return '未知地区';
    return _names[c] ?? c;
  }

  /// PNG 国旗（与面板 flagcdn 备用一致）。
  static String? imageUrl(String? code, {int width = 48}) {
    final c = normalize(code);
    if (c == null) return null;
    const size = '48x36';
    return 'https://flagcdn.com/$size/${c.toLowerCase()}.png';
  }
}
