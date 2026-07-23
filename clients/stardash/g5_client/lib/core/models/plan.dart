class PlanPeriod {
  PlanPeriod({
    required this.key,
    required this.label,
    required this.amountCents,
    required this.monthlyCents,
    this.savePercent,
  });

  factory PlanPeriod.fromJson(Map<String, dynamic> json) {
    return PlanPeriod(
      key: json['key'] as String,
      label: json['label'] as String,
      amountCents: json['amount_cents'] as int? ?? 0,
      monthlyCents: json['monthly_cents'] as int? ?? 0,
      savePercent: json['save_percent'] as int?,
    );
  }

  final String key;
  final String label;
  final int amountCents;
  final int monthlyCents;
  final int? savePercent;

  double get amountYuan => amountCents / 100;
}

class Plan {
  Plan({
    required this.id,
    required this.name,
    this.content,
    required this.transferEnable,
    this.deviceLimit,
    required this.speedLimit,
    required this.renew,
    required this.isTrial,
    required this.periods,
  });

  factory Plan.fromJson(Map<String, dynamic> json) {
    final periodsRaw = json['periods'] as List<dynamic>? ?? [];
    return Plan(
      id: json['id'] as int,
      name: json['name'] as String,
      content: json['content'] as String?,
      transferEnable: json['transfer_enable'] as int? ?? 0,
      deviceLimit: json['device_limit'] as int?,
      speedLimit: json['speed_limit'] as int? ?? 0,
      renew: json['renew'] as bool? ?? true,
      isTrial: json['is_trial'] as bool? ?? false,
      periods: periodsRaw
          .map((e) => PlanPeriod.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }

  final int id;
  final String name;
  final String? content;
  final int transferEnable;
  final int? deviceLimit;
  final int speedLimit;
  final bool renew;
  final bool isTrial;
  final List<PlanPeriod> periods;

  String get transferLabel {
    if (transferEnable <= 0) return '不限流量';
    return '${(transferEnable / (1024 * 1024 * 1024)).toStringAsFixed(0)} GB';
  }
}
