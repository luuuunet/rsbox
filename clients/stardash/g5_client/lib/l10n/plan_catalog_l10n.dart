import 'app_localizations.dart';

/// 将服务端套餐/周期文案映射为当前客户端语言。
extension PlanCatalogL10n on AppLocalizations {
  /// 周期展示（优先用 [periodKey]，并修复 `plan.period.month` 等未翻译 key）。
  String localizePeriod(String periodKey, [String? serverLabel]) {
    final key = _resolvePeriodKey(periodKey, serverLabel);
    return switch (key) {
      'month' => periodMonth,
      'quarter' => periodQuarter,
      'half_year' => periodHalfYear,
      'year' => periodYear,
      'two_year' => periodTwoYear,
      'three_year' => periodThreeYear,
      'onetime' => periodOnetime,
      _ => _humanizeServerLabel(serverLabel, periodKey),
    };
  }

  /// 套餐名称（常见预设名多语言；自定义名称原样显示）。
  String localizePlanName(String? raw) {
    if (raw == null || raw.trim().isEmpty) return plan;
    final trimmed = raw.trim();
    for (final entry in _planNameAliasEntries) {
      if (entry.aliases.contains(trimmed)) {
        return _planNameByKey(entry.key);
      }
    }
    return trimmed;
  }

  /// 套餐说明：先尝试整段映射，再替换常见短语。
  String localizePlanContent(String? raw) {
    if (raw == null || raw.trim().isEmpty) return '';
    final trimmed = raw.trim();
    for (final entry in _planContentAliasEntries) {
      if (entry.aliases.contains(trimmed)) {
        return _planContentByKey(entry.key);
      }
    }
    return trimmed
        .replaceAll('不限流量', unlimitedTraffic)
        .replaceAll('不限流量。', unlimitedTraffic)
        .replaceAll('Unlimited traffic', unlimitedTraffic)
        .replaceAll('Unlimited', unlimitedTraffic);
  }

  /// 流量配额展示。
  String formatPlanTraffic(int transferEnableBytes) {
    if (transferEnableBytes <= 0) return unlimitedTraffic;
    final gb = transferEnableBytes / (1024 * 1024 * 1024);
    final n = gb >= 10 ? gb.round() : gb.toStringAsFixed(gb == gb.roundToDouble() ? 0 : 1);
    return planTrafficGb('$n');
  }

  String _planNameByKey(String key) => switch (key) {
        'basic' => planNameBasic,
        'standard' => planNameStandard,
        'premium' => planNamePremium,
        'pro' => planNamePro,
        'trial' => planNameTrial,
        _ => plan,
      };

  String _planContentByKey(String key) => switch (key) {
        'unlimited' => planContentUnlimited,
        _ => '',
      };

  String _humanizeServerLabel(String? serverLabel, String periodKey) {
    if (serverLabel == null || serverLabel.isEmpty) return periodKey;
    if (serverLabel.startsWith('plan.period.')) {
      final k = serverLabel.substring('plan.period.'.length);
      return localizePeriod(k, null);
    }
    return serverLabel;
  }

  static String _resolvePeriodKey(String key, String? label) {
    const known = {
      'month',
      'quarter',
      'half_year',
      'year',
      'two_year',
      'three_year',
      'onetime',
    };
    if (known.contains(key)) return key;

    if (label != null && label.startsWith('plan.period.')) {
      final k = label.substring('plan.period.'.length);
      if (known.contains(k)) return k;
    }

    if (label != null) {
      final normalized = label.trim().toLowerCase();
      final fromLabel = _periodLabelToKey[normalized];
      if (fromLabel != null) return fromLabel;
    }

    return key;
  }
}

class _AliasEntry {
  const _AliasEntry(this.key, this.aliases);
  final String key;
  final Set<String> aliases;
}

const _planNameAliasEntries = [
  _AliasEntry('basic', {
    '基础套餐',
    '基础',
    '基礎套餐',
    '基礎',
    'Basic Plan',
    'Basic',
    'ベーシック',
    'ベーシックプラン',
    '베이직',
    'Plan básico',
    'Forfait de base',
  }),
  _AliasEntry('standard', {
    '标准套餐',
    '标准',
    '標準套餐',
    '標準',
    'Standard Plan',
    'Standard',
    'スタンダード',
    '스탠다드',
    'Plan estándar',
    'Forfait standard',
  }),
  _AliasEntry('premium', {
    '高级套餐',
    '高级',
    '高級套餐',
    '高級',
    'Premium Plan',
    'Premium',
    'プレミアム',
    '프리미엄',
    'Plan premium',
    'Forfait premium',
  }),
  _AliasEntry('pro', {
    '专业套餐',
    '专业',
    '專業套餐',
    '專業',
    'Pro Plan',
    'Pro',
    'プロ',
    '프로',
  }),
  _AliasEntry('trial', {
    '试用套餐',
    '试用',
    '試用套餐',
    '試用',
    '体验套餐',
    '體驗套餐',
    'Trial Plan',
    'Trial',
    'トライアル',
    '체험',
    'Prueba',
    'Essai',
  }),
];

const _planContentAliasEntries = [
  _AliasEntry('unlimited', {
    '不限流量',
    '不限流量。',
    'Unlimited traffic',
    'Unlimited',
    '無制限流量',
    'トラフィック無制限',
  }),
];

const _periodLabelToKey = {
  'monthly': 'month',
  'month': 'month',
  '月付': 'month',
  '月付费': 'month',
  '月付費': 'month',
  'mensual': 'month',
  'mensuel': 'month',
  '월간': 'month',
  '月額': 'month',
  'quarterly': 'quarter',
  'quarter': 'quarter',
  '季付': 'quarter',
  '季付费': 'quarter',
  'trimestral': 'quarter',
  'semi-annual': 'half_year',
  'semi-annual payment': 'half_year',
  'half_year': 'half_year',
  '半年付': 'half_year',
  '半年付费': 'half_year',
  'annual': 'year',
  'yearly': 'year',
  'year': 'year',
  '年付': 'year',
  '年付费': 'year',
  '年付費': 'year',
  'anual': 'year',
  'annuel': 'year',
  '2-year': 'two_year',
  'two_year': 'two_year',
  '两年付': 'two_year',
  '兩年付': 'two_year',
  '3-year': 'three_year',
  'three_year': 'three_year',
  '三年付': 'three_year',
  'one-time': 'onetime',
  'one-time payment': 'onetime',
  'onetime': 'onetime',
  '一次性': 'onetime',
  'plan.period.month': 'month',
  'plan.period.quarter': 'quarter',
  'plan.period.half_year': 'half_year',
  'plan.period.year': 'year',
  'plan.period.two_year': 'two_year',
  'plan.period.three_year': 'three_year',
  'plan.period.onetime': 'onetime',
};
